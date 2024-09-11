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
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use pkgsrc::distinfo::Distinfo;
use rayon::prelude::*;
use std::env;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;
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
        let mut files: Vec<FetchFile> = vec![];

        let distinfo = match &self.distinfo {
            Some(s) => Distinfo::from_bytes(
                &fs::read(s).expect("unable to read distinfo"),
            ),
            None => Distinfo::new(),
        };

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
            // filepath distdir site [site ...]
            for line in reader.lines() {
                let line = line?;
                let v: Vec<&str> = line.split_whitespace().collect();
                if v.len() < 3 {
                    eprintln!("Invalid input: {}", line);
                    return Ok(1);
                }
                let filepath = PathBuf::from(v[0]);
                let distdir = PathBuf::from(v[1]);
                let sites: Vec<String> =
                    v[2..].iter().map(|s| s.to_string()).collect();

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

        let progress = MultiProgress::new();

        files.par_iter_mut().for_each(|file| {
            if fetch_and_verify(file, &distinfo, &progress).is_err() {
                file.status = false;
            }
        });

        let mut rc = 0;
        for f in &files {
            if !f.status {
                rc = 1;
                break;
            }
        }

        Ok(rc)
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
    if let Some(s) = site.strip_prefix("-") {
        url.push_str(s);
    } else {
        url.push_str(site);
        if !site.ends_with("/") {
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
    file: &FetchFile,
    distinfo: &Distinfo,
    progress: &MultiProgress,
) -> Result<(), FetchError> {
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
        match distinfo.verify_size(&file_name) {
            Ok(_) => return Ok(()),
            Err(_) => fs::remove_file(&file_name)?,
        }
    }

    let style = ProgressStyle::with_template(
        "[{msg:20!}] {bar:40.cyan/blue} {binary_bytes:>7}/{binary_total_bytes:7}",
    )
    .unwrap()
    .progress_chars("##-");

    'nextsite: for site in &file.sites {
        let url = url_from_site(site, &file.filename);
        match reqwest::blocking::get(&url) {
            Ok(mut body) => {
                if !&body.status().is_success() {
                    eprintln!("Unable to fetch {}: {}", url, &body.status());
                    continue;
                }
                let pb = progress
                    .add(ProgressBar::new(body.content_length().unwrap_or(0)));
                pb.set_message(file.filename.clone());
                pb.set_style(style.clone());
                let file = File::create(&file_name)?;
                body.copy_to(&mut pb.wrap_write(&file))?;
                pb.tick();
                pb.finish();
                progress.remove(&pb);
                /*
                 * Perform file checks.
                 */
                for result in distinfo.verify_checksums(&file_name) {
                    if let Err(e) = result {
                        eprintln!("Failed to fetch {url}");
                        eprintln!("{e}");
                        continue 'nextsite;
                    }
                }
                return Ok(());
            }
            Err(e) => {
                /*
                 * Some issue during connection.  We decend twice through
                 * source() to get to the underlying hyper error message as
                 * the reqwest "Connect" is all but useless.  There's probably
                 * a simpler way to do this but I couldn't find it.
                 */
                if let Some(r) = e.source() {
                    if let Some(h) = r.source() {
                        eprintln!("Unable to fetch {}: {}", url, h);
                    } else {
                        eprintln!("Unable to fetch {}: {}", url, r);
                    }
                } else {
                    eprintln!("Unable to fetch {}: {}", url, e);
                }
            }
        }
    }
    Err(FetchError::NotFound)
}
