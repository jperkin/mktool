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

use clap::Args;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use pkgsrc::distinfo::Distinfo;
use std::env;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
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
    jobs: Option<u64>,
}

#[derive(Clone, Debug)]
struct FetchFile {
    filename: String,
    distdir: String,
    sites: Vec<String>,
    status: bool,
}

#[derive(Error, Debug)]
pub enum FetchError {
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("Unable to fetch file: {0}")]
    NotFound(String),
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
            // filename distdir site ...
            for line in reader.lines() {
                let line = line?;
                if line.split_whitespace().count() < 3 {
                    eprintln!("Invalid input: {}", line);
                    return Ok(1);
                }
                let mut line = line.split_whitespace();
                let file = line.next().expect("").to_string();
                let dir = line.next().expect("").to_string();
                let sites: Vec<String> =
                    line.map(|x| x.to_string()).collect::<Vec<String>>();

                files.push(FetchFile {
                    filename: file,
                    distdir: dir,
                    sites,
                    status: true,
                });
            }
        }

        let mut threads = vec![];
        let active_threads = Arc::new(Mutex::new(0));
        let default_threads = 4;
        let max_threads = match self.jobs {
            Some(n) => n,
            None => match env::var("MKTOOL_JOBS") {
                Ok(n) => n.parse::<u64>().unwrap_or(default_threads),
                Err(_) => default_threads,
            },
        };

        let progress = MultiProgress::new();

        let files: Vec<_> =
            files.into_iter().map(|s| Arc::new(Mutex::new(s))).collect();

        for f in &files {
            let active_threads = Arc::clone(&active_threads);
            let f = Arc::clone(f);
            let p = progress.clone();
            let d = distinfo.clone();
            loop {
                {
                    let mut active = active_threads.lock().unwrap();
                    if *active < max_threads {
                        *active += 1;
                        break;
                    }
                }
                thread::yield_now();
            }
            let t = thread::spawn(move || {
                let mut f = f.lock().unwrap();
                if fetch_and_verify(&f, &d, &p).is_err() {
                    f.status = false;
                }
                {
                    let mut active = active_threads.lock().unwrap();
                    *active -= 1;
                }
            });
            threads.push(t);
        }
        for t in threads {
            t.join().unwrap();
        }
        /* Unwrap Arc/Mutex */
        let files: Vec<_> = files
            .into_iter()
            .map(|arc_mutex| {
                Arc::try_unwrap(arc_mutex)
                    .ok()
                    .unwrap()
                    .into_inner()
                    .unwrap()
            })
            .collect();

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
    file_name.push(&file.filename);

    let style = ProgressStyle::with_template(
        "[{msg:20!}] {bar:40.cyan/blue} {binary_bytes:>7}/{binary_total_bytes:7}",
    )
    .unwrap()
    .progress_chars("##-");

    for site in &file.sites {
        let fname = file.filename.clone();
        let url = url_from_site(site, &file.filename);
        match reqwest::blocking::get(&url) {
            Ok(mut body) => {
                if !&body.status().is_success() {
                    eprintln!("Unable to fetch {}: {}", url, &body.status());
                    continue;
                }
                let pb = progress
                    .add(ProgressBar::new(body.content_length().unwrap_or(0)));
                pb.set_message(fname);
                pb.set_style(style.clone());
                let file = File::create(&file_name)?;
                body.copy_to(&mut pb.wrap_write(&file))?;
                pb.tick();
                pb.finish();
                progress.remove(&pb);
                /*
                 * Perform file checks.
                 */
                match distinfo.check_file(&file_name) {
                    Ok(_) => return Ok(()),
                    Err(e) => {
                        eprintln!("{e}");
                        continue;
                    }
                }
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
    Err(FetchError::NotFound(file.filename.clone()))
}
