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
use pkgsrc::digest::Digest;
use pkgsrc::distinfo::{Distinfo, DistinfoError, Entry};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Args, Debug)]
pub struct CheckSum {
    #[arg(short = 'a', value_name = "algorithm")]
    #[arg(help = "Only verify checksums for the specified algorithm")]
    algorithm: Option<String>,

    #[arg(short = 'I', value_name = "input")]
    #[arg(help = "Read files from input instead of command line arguments")]
    input: Option<PathBuf>,

    #[arg(short = 'j', value_name = "jobs")]
    #[arg(help = "Maximum number of threads (or \"MKTOOL_JOBS\" env var)")]
    jobs: Option<u64>,

    #[arg(short = 'p', default_value = "false")]
    #[arg(help = "Operate in patch mode")]
    patchmode: bool,

    #[arg(short = 's', value_name = "suffix")]
    #[arg(help = "Strip the specified suffix from file names")]
    stripsuffix: Option<String>,

    #[arg(value_name = "distinfo")]
    #[arg(help = "Path to a distinfo file to verify against")]
    distinfo: PathBuf,

    #[arg(value_name = "file")]
    #[arg(help = "List of files to verify against distinfo")]
    files: Vec<PathBuf>,
}

#[derive(Debug)]
struct CheckResult {
    entry: Entry,
    results: Mutex<Vec<Result<Digest, DistinfoError>>>,
}

impl CheckSum {
    pub fn run(&self) -> Result<i32, Box<dyn std::error::Error>> {
        /*
         * Read in the supplied "distinfo" file.
         */
        let distinfo = match fs::read(&self.distinfo) {
            Ok(s) => s,
            Err(_) => {
                /* Compatible output/exit status with checksum.awk */
                eprintln!(
                    "checksum: distinfo file missing: {}",
                    &self.distinfo.display()
                );
                return Ok(3);
            }
        };
        let distinfo = Distinfo::from_bytes(&distinfo);

        /*
         * Add files passed in via -I (supporting stdin if set to "-"), and
         * then those passed on the command line, storing unique entries in
         * the inputfiles HashSet.
         */
        let mut inputfiles: HashSet<PathBuf> = HashSet::new();

        if let Some(infile) = &self.input {
            let reader: Box<dyn io::BufRead> = match infile.to_str() {
                Some("-") => Box::new(io::stdin().lock()),
                Some(f) => Box::new(BufReader::new(fs::File::open(f)?)),
                None => {
                    eprintln!(
                        "ERROR: File '{}' is not valid unicode.",
                        infile.display()
                    );
                    std::process::exit(1);
                }
            };
            for line in reader.lines() {
                let line = line?;
                if let Some(suffix) = &self.stripsuffix {
                    match line.strip_suffix(suffix) {
                        Some(s) => inputfiles.insert(PathBuf::from(s)),
                        None => inputfiles.insert(PathBuf::from(line)),
                    };
                } else {
                    inputfiles.insert(PathBuf::from(line));
                }
            }
        }
        for file in &self.files {
            if let Some(suffix) = &self.stripsuffix {
                match file
                    .to_str()
                    .expect("filename is not valid UTF-8")
                    .strip_suffix(suffix)
                {
                    Some(s) => inputfiles.insert(PathBuf::from(s)),
                    None => inputfiles.insert(PathBuf::from(file)),
                };
            } else {
                inputfiles.insert(PathBuf::from(file));
            }
        }

        /*
         * No input files, return early.
         */
        if inputfiles.is_empty() {
            return Ok(0);
        }

        /*
         * Iterate through all input files, adding to checkfiles if found in
         * distinfo.  Any entries left in inputfiles are later printed as
         * missing.
         */
        let mut checkfiles: HashSet<Entry> = HashSet::new();
        let mut remove: Vec<PathBuf> = Vec::new();
        for file in &inputfiles {
            let entry = match distinfo.find_entry(file) {
                Ok(e) => e,
                Err(_) => continue,
            };
            checkfiles.insert(entry.clone());
            remove.push(file.to_path_buf());
        }
        for r in remove {
            inputfiles.remove(&r);
        }

        /*
         * If a single algorithm is requested then only match it.
         */
        let mut single_digest: Option<Digest> = None;
        if let Some(a) = &self.algorithm {
            single_digest = Some(Digest::from_str(a)?);
        }

        /*
         * Convert checkfiles into a Arc Vec, wrapping results in a Mutex
         * so that we can process them in parallel.
         */
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
        let checkfiles: Vec<Arc<CheckResult>> = checkfiles
            .into_iter()
            .map(|e| {
                Arc::new(CheckResult {
                    entry: e,
                    results: Mutex::new(vec![]),
                })
            })
            .collect();

        for file in &checkfiles {
            let active_threads = Arc::clone(&active_threads);
            let file = Arc::clone(file);
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
            let thread = thread::spawn(move || {
                let mut res = file.results.lock().unwrap();
                match single_digest {
                    Some(digest) => {
                        *res = vec![file
                            .entry
                            .verify_checksum(&file.entry.filename, digest)]
                    }
                    None => {
                        *res = file.entry.verify_checksums(&file.entry.filename)
                    }
                };
                {
                    let mut active = active_threads.lock().unwrap();
                    *active -= 1;
                }
            });
            threads.push(thread);
        }
        for thread in threads {
            thread.join().unwrap();
        }

        /*
         * Unwrap processed checkfiles back into a plain Vec, sorted by entry
         * filename, to ease processing.
         */
        let mut checkfiles: Vec<CheckResult> = checkfiles
            .into_iter()
            .map(|a| Arc::into_inner(a).unwrap())
            .collect();
        checkfiles.sort_by(|a, b| a.entry.filename.cmp(&b.entry.filename));

        /*
         * We have processed everything, print results and return compatible
         * exit status.  Output and order should match checksum.awk.
         */
        let mut rv = 0;
        for file in checkfiles {
            for result in file.results.lock().unwrap().iter() {
                match result {
                    Ok(digest) => println!(
                        "=> Checksum {} OK for {}",
                        digest,
                        file.entry.filename.display()
                    ),
                    Err(DistinfoError::Checksum(path, digest, _, _)) => {
                        eprintln!(
                            "checksum: Checksum {} mismatch for {}",
                            digest,
                            path.display()
                        );
                        /* checksum.awk bails on first mismatch */
                        return Ok(1);
                    }
                    Err(DistinfoError::MissingChecksum(path, digest)) => {
                        eprintln!(
                            "checksum: No {} checksum recorded for {}",
                            digest,
                            path.display()
                        );
                        rv = 2;
                    }
                    Err(e) => eprintln!("ERROR: {e}"),
                }
            }
        }
        /*
         * checksum.awk prints missing files in arbitrary order.  We differ
         * in behaviour here and ensure they are sorted, mainly because it
         * ensures test results are stable.
         */
        let mut missing: Vec<PathBuf> = inputfiles.into_iter().collect();
        missing.sort();
        for file in missing {
            if let Some(digest) = single_digest {
                eprintln!(
                    "checksum: No {} checksum recorded for {}",
                    digest,
                    file.display()
                );
            } else {
                eprintln!(
                    "checksum: No checksum recorded for {}",
                    file.display()
                );
            }
            rv = 2;
        }

        Ok(rv)
    }
}
