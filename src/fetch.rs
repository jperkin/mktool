/*
 * Copyright (c) 2024 Jonathan Perkin <jonathan@perkin.org.uk>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

use crate::MKTOOL_DEFAULT_THREADS;
use clap::Args;
use indicatif::{HumanBytes, HumanDuration, ProgressBar, ProgressStyle};
use pkgsrc::distinfo::Distinfo;
use rayon::prelude::*;
use reqwest::blocking::Client;
use std::env;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;
use std::time::Instant;
use thiserror::Error;

#[derive(Args, Debug)]
pub struct Fetch {
    #[arg(short, value_name = "distdir", default_value = ".")]
    #[arg(help = "Directory where distfiles are stored")]
    distdir: PathBuf,

    #[arg(short = 'f', value_name = "distinfo")]
    #[arg(help = "Path to a distinfo file containing checksums")]
    distinfo: Option<PathBuf>,

    #[arg(short = 'I', value_name = "input")]
    #[arg(help = "Read files from input")]
    input: Option<PathBuf>,

    #[arg(short = 'j', value_name = "jobs")]
    #[arg(help = "Maximum number of threads (or \"MKTOOL_JOBS\" env var)")]
    jobs: Option<usize>,
}

#[derive(Clone, Debug)]
struct FetchFile {
    filepath: PathBuf,
    filename: String,
    distdir: PathBuf,
    sites: Vec<String>,
    status: bool,
}

#[derive(Error, Debug)]
pub enum FetchError {
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("Unable to fetch file")]
    NotFound,
}

impl Fetch {
    pub fn run(&self) -> Result<i32, FetchError> {
        let started = Instant::now();
        let mut files: Vec<FetchFile> = vec![];

        let distinfo = self.distinfo.as_ref().map(|s| {
            Distinfo::from_bytes(&fs::read(s).expect("unable to read distinfo"))
        });

        if let Some(input) = &self.input {
            let reader: Box<dyn io::BufRead> = match input.to_str() {
                Some("-") => Box::new(io::stdin().lock()),
                Some(f) => Box::new(BufReader::new(File::open(f)?)),
                None => {
                    eprintln!(
                        "ERROR: File '{}' is not valid unicode.",
                        input.display()
                    );
                    std::process::exit(1);
                }
            };
            // filepath distdir [site ...]
            for line in reader.lines() {
                let line = line?;
                let v: Vec<&str> = line.split_whitespace().collect();
                if v.len() < 2 {
                    eprintln!("fetch: Invalid input: {}", line);
                    return Ok(1);
                }
                let filepath = PathBuf::from(v[0]);
                let distdir = PathBuf::from(v[1]);
                /*
                 * In some cases no site will be specified, e.g. Oracle Java
                 * files that the user needs to fetch manually.
                 */
                let sites = v
                    .get(2..)
                    .unwrap_or(&[])
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>();

                /*
                 * While technically we could support non-UTF-8 paths, and try
                 * to do so by using PathBufs, we are currently restricted by
                 * both indicatif and reqwest requiring String or str.  So for
                 * now just give up if we can't convert.
                 */
                let filename = String::from(
                    filepath
                        .file_name()
                        .expect("unable to extract filename")
                        .to_str()
                        .unwrap_or("blah"),
                );
                files.push(FetchFile {
                    filepath,
                    filename,
                    distdir,
                    sites,
                    status: true,
                });
            }
        }

        /*
         * Set up rayon threadpool.  -j argument has highest precedence, then
         * MKTOOLS_JOBS environment variable, finally MKTOOL_DEFAULT_THREADS.
         */
        let nthreads = match self.jobs {
            Some(n) => n,
            None => match env::var("MKTOOL_JOBS") {
                Ok(n) => n.parse::<usize>().unwrap_or(MKTOOL_DEFAULT_THREADS),
                Err(_) => MKTOOL_DEFAULT_THREADS,
            },
        };
        rayon::ThreadPoolBuilder::new()
            .num_threads(nthreads)
            .build_global()
            .unwrap();

        /*
         * Set up the progress bar.
         */
        let style = ProgressStyle::with_template(
            "{prefix:>12} [{bar:57}] {binary_bytes:>7}/{binary_total_bytes:7}",
        )
        .unwrap()
        .progress_chars("=> ");
        let progress =
            ProgressBar::new(0).with_prefix("Downloading").with_style(style);

        /*
         * Disable the Referer: header, this appears to cause problems with
         * redirect handling when downloading from SourceForge.
         */
        let client =
            reqwest::blocking::Client::builder().referer(false).build()?;

        files.par_iter_mut().for_each(|file| {
            if fetch_and_verify(&client, file, &distinfo, &progress).is_err() {
                file.status = false;
            }
        });

        progress.finish_and_clear();

        let mut rv = 0;
        for f in &files {
            if !f.status {
                rv = 1;
                break;
            }
        }

        /*
         * Only print the final message if we downloaded something and
         * everything was a success.
         */
        if progress.length() > Some(0) && rv == 0 {
            let dsize = progress.length().unwrap();
            let dtime = started.elapsed();
            println!(
                "Downloaded {} in {} ({}/s)",
                HumanBytes(dsize),
                HumanDuration(dtime),
                HumanBytes(dsize / dtime.as_millis() as u64 * 1000)
            );
        }

        Ok(rv)
    }
}

/*
 * Parse a site and filename, and return the full URL.
 *
 * URLs that start with a "-" mean fetch the file directly, otherwise
 * append the target filename to the site URL.
 */
fn url_from_site(site: &str, filename: &str) -> String {
    let mut url = String::new();
    if let Some(s) = site.strip_prefix('-') {
        url.push_str(s);
    } else {
        url.push_str(site);
        if !site.ends_with('/') {
            url.push('/');
        }
        url.push_str(filename);
    };
    url
}

/*
 * Attempt to download a file from a list of sites, and verify against the
 * listed checksums.
 */
fn fetch_and_verify(
    client: &Client,
    file: &FetchFile,
    distinfo: &Option<Distinfo>,
    progress: &ProgressBar,
) -> Result<u64, FetchError> {
    // Set the target filename
    let mut file_name = PathBuf::from(&file.distdir);
    file_name.push(&file.filepath);

    /*
     * Create all necessary directories.
     */
    if let Some(dir) = file_name.parent() {
        fs::create_dir_all(dir)?;
    }

    /*
     * There's no support for resume yet.  If the file already exists and
     * matches the correct size then assume it's ok (checksum will later
     * verify that it is), otherwise remove and retry.
     */
    if file_name.exists() {
        if let Some(di) = distinfo {
            match di.verify_size(&file_name) {
                Ok(s) => return Ok(s),
                Err(_) => fs::remove_file(&file_name)?,
            }
        } else {
            return Ok(file_name.metadata()?.len());
        }
    }

    /*
     * If we cannot determine the length of the remote file (e.g. no
     * Content-Length header) then fall back to the size (if available) that
     * we have recorded in distinfo.  If neither are available then we have
     * no choice but to leave it at zero.
     */
    let expected_size = if let Some(di) = distinfo {
        match di.get_distfile(&file.filepath) {
            Some(e) => e.size.unwrap_or(0),
            None => 0,
        }
    } else {
        0
    };

    /*
     * Update progress output, with simple output for non-ttys.  Set the
     * progress bar length to the expected size if available, as this helps
     * show a useful progress bar while potential redirects are followed.
     */
    progress.inc_length(expected_size);
    if progress.is_hidden() {
        println!("Fetching {}", &file.filename);
    } else {
        progress.println(format!("{:>12} {}", "Fetching", &file.filename));
    }

    'nextsite: for site in &file.sites {
        let url = url_from_site(site, &file.filename);
        match client.get(&url).send() {
            Ok(mut body) => {
                /*
                 * If we don't have an expected size from distinfo then update
                 * the progress bar with the content length, if available.
                 */
                if expected_size == 0 {
                    if let Some(len) = body.content_length() {
                        progress.inc_length(len);
                    }
                }

                if !&body.status().is_success() {
                    progress.suspend(|| {
                        eprintln!(
                            "Unable to fetch {}: {}",
                            url,
                            &body.status()
                        );
                    });
                    continue;
                }

                /*
                 * Write the file and perform distinfo checks.
                 */
                let file = File::create(&file_name)?;
                body.copy_to(&mut progress.wrap_write(&file))?;
                if let Some(di) = distinfo {
                    for result in di.verify_checksums(&file_name) {
                        if let Err(e) = result {
                            progress.suspend(|| {
                                eprintln!("Verification failed for {url}: {e}");
                            });
                            continue 'nextsite;
                        }
                    }
                }
                return Ok(file.metadata()?.len());
            }
            Err(e) => {
                /*
                 * Some issue during connection.  We decend twice through
                 * source() to get to the underlying hyper error message as
                 * the reqwest "Connect" is all but useless.  There's probably
                 * a simpler way to do this but I couldn't find it.
                 */
                let errmsg = if let Some(reqwest) = e.source() {
                    if let Some(hyper) = reqwest.source() {
                        format!("Unable to fetch {}: {}", url, hyper)
                    } else {
                        format!("Unable to fetch {}: {}", url, reqwest)
                    }
                } else {
                    format!("Unable to fetch {}: {}", url, e)
                };
                progress.suspend(|| {
                    eprintln!("{}", errmsg);
                });
            }
        }
    }
    Err(FetchError::NotFound)
}
