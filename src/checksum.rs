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

use crate::distinfo::{DistInfoEntry, DistInfoType, HashEntry};
use clap::Args;
use pkgsrc::digest::Digest;
use std::collections::HashMap;
use std::fs;
use std::io::BufRead;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Args, Debug)]
pub struct Checksum {
    #[arg(short = 'a', value_name = "algorithm")]
    #[arg(help = "Only verify checksums for the specified algorithm")]
    algorithm: Option<String>,

    #[arg(short = 'j', value_name = "jobs", default_value = "4")]
    #[arg(help = "Number of parallel jobs to process at a time")]
    jobs: u64,

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

impl Checksum {
    pub fn run(&self) -> Result<i32, Box<dyn std::error::Error>> {
        /*
         * List of distfiles to check.
         */
        let mut distfiles: HashMap<String, DistInfoEntry> = HashMap::new();

        let di_type = if self.patchmode {
            DistInfoType::Patch
        } else {
            DistInfoType::Distfile
        };

        /*
         * Iterate over files passed on the command line, optionally stripping
         * a suffix from them, and storing in the "distfiles" HashMap.
         */
        for file in &self.files {
            let f = file
                .file_name()
                .ok_or("Input is not a filename")?
                .to_str()
                .ok_or("Filename is not valid unicode")?;
            let f = match &self.stripsuffix {
                Some(s) => match f.strip_suffix(s) {
                    Some(s) => s,
                    None => f,
                },
                None => f,
            };
            distfiles.insert(
                f.to_string(),
                DistInfoEntry {
                    filetype: di_type.clone(),
                    filepath: file.clone(),
                    processed: false,
                    ..Default::default()
                },
            );
        }

        /*
         * Iterate over the "distinfo" file, selecting lines that we are
         * interested in.
         */
        let distinfo = match fs::read(&self.distinfo) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "ERROR: Could not open distinfo '{}': {}",
                    &self.distinfo.display(),
                    e
                );
                // Return code compatible with checksum.awk
                return Ok(3);
            }
        };
        /*
         * Save this first pass to simplify the second pass at the end.
         */
        let mut di_lines: Vec<(Digest, String, String)> = vec![];
        for (i, line) in distinfo.lines().enumerate() {
            /*
             * Skip $NetBSD$ header and blank line.
             */
            if i < 2 {
                continue;
            }

            let line = line?;
            if line.starts_with("#") {
                continue;
            }

            /*
             * All lines should be of the form "alg (file) = hash".
             */
            let items = line.split(" ").collect::<Vec<_>>();
            if items.len() != 4 {
                continue;
            }

            let algorithm = items[0];
            let distfile = items[1].split(&['(', ')']).collect::<String>();
            let checksum = items[3];

            /*
             * Skip "Size" and "IGNORE" (legacy) lines.
             */
            if algorithm == "Size" || checksum == "IGNORE" {
                continue;
            }

            /*
             * Get the full path for the file from the distfiles HashMap,
             * using the entry from distinfo as the key.
             */
            let df = match distfiles.get_mut(&distfile) {
                Some(s) => s,
                None => continue,
            };

            /*
             * If a single algorithm is requested then match it, otherwise
             * skip.
             */
            if let Some(a) = &self.algorithm {
                let d = Digest::from_str(a)?;
                if algorithm != a {
                    continue;
                }
                df.hashes.push(HashEntry {
                    digest: d,
                    hash: String::new(),
                });
            } else {
                let d = Digest::from_str(algorithm)?;
                df.hashes.push(HashEntry {
                    digest: d,
                    hash: String::new(),
                });
            }

            /*
             * Mark as "seen", using processed flag, and save to di_lines for
             * final pass.
             */
            df.processed = true;
            di_lines.push((
                Digest::from_str(algorithm)?,
                distfile.to_string(),
                checksum.to_string(),
            ));
        }

        /*
         * Convert distfiles HashMap into a Vec, wrapping each in a Mutex, so
         * that we can process them in parallel.
         */
        let mut threads = vec![];
        let active_threads = Arc::new(Mutex::new(0));
        let max_threads = self.jobs;
        let di_vec: Vec<_> = distfiles
            .into_values()
            .map(|v| Arc::new(Mutex::new(v)))
            .collect();

        for d in &di_vec {
            let active_threads = Arc::clone(&active_threads);
            let d = Arc::clone(d);
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
                let mut d = d.lock().unwrap();
                // XXX: Return correct Result here.
                let _ = d.calculate();
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

        /*
         * Convert distfiles back into a HashMap.
         */
        let distfiles: HashMap<String, DistInfoEntry> = di_vec
            .into_iter()
            .map(|arc_mutex| {
                Arc::try_unwrap(arc_mutex)
                    .ok()
                    .unwrap()
                    .into_inner()
                    .unwrap()
            })
            .map(|p| {
                (
                    p.filepath
                        .file_name()
                        .expect("Input is not a filename")
                        .to_str()
                        .expect("Filename is not valid unicode")
                        .to_string(),
                    p,
                )
            })
            .collect();

        /*
         * We have processed everything, print results.  This is done in
         * multiple passes to keep the output order compatible with
         * checksum.awk.
         */
        for (alg, file, hash) in di_lines {
            let df = match distfiles.get(&file) {
                Some(s) => s,
                None => continue,
            };
            /*
             * Find correct digest entry.
             */
            let mut found = false;
            for h in &df.hashes {
                if h.digest == alg && h.hash == hash {
                    println!("=> Checksum {} OK for {}", alg, file);
                    found = true;
                    break;
                }
            }
            if !found {
                eprintln!("checksum: Checksum {} mismatch for {}", alg, file);
                return Ok(1);
            }
        }
        let mut rv = 0;
        for (k, v) in distfiles {
            if !v.processed {
                if let Some(a) = &self.algorithm {
                    eprintln!("checksum: No {} checksum recorded for {}", a, k);
                } else {
                    eprintln!("checksum: No checksum recorded for {}", k);
                }
                rv = 2;
            }
        }
        Ok(rv)
    }
}
