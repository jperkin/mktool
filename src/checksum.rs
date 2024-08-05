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
use std::collections::HashMap;
use std::fs;
use std::io::BufRead;
use std::path::PathBuf;
use std::str::FromStr;

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
        let mut distfiles: HashMap<String, PathBuf> = HashMap::new();

        /*
         * Iterate over files passed on the command line, optionally stripping
         * a suffix from them, and storing in the "distfiles" HashMap.
         */
        for file in &self.files {
            // XXX: return a proper Result here.
            let f = file.file_name().expect("unable to extract filename");
            let f = f.to_str().expect("unable to convert filename to str");
            let f = match &self.stripsuffix {
                Some(s) => match f.strip_suffix(s) {
                    Some(s) => s,
                    None => f,
                },
                None => f,
            };
            distfiles.insert(f.to_string(), file.clone());
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
                return Ok(3);
            }
        };
        for (i, line) in distinfo.lines().enumerate() {
            /*
             * Skip $NetBSD$ header and blank line.
             */
            if i < 2 {
                continue;
            }

            let line = line?;

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
             * If a single algorithm is requested then match it.
             */
            if let Some(a) = &self.algorithm {
                // Be kind and verify algorithm is valid.
                let _ = Digest::from_str(a)?;
                if algorithm != a {
                    continue;
                }
            }

            /*
             * Get the full path for the file from the distfiles HashMap,
             * using the entry from distinfo as the key.
             */
            let filepath = match distfiles.get(&distfile) {
                Some(s) => s,
                None => continue,
            };

            /*
             * Calculate digest based on whether a distfile or patch.
             */
            let mut f = fs::File::open(filepath)?;
            let d = Digest::from_str(algorithm)?;
            let h = match self.patchmode {
                true => d.hash_patch(&mut f)?,
                false => d.hash_file(&mut f)?,
            };

            if h == checksum {
                println!("=> Checksum {} OK for {}", algorithm, distfile);
            } else {
                eprintln!(
                    "checksum: Checksum {} mismatch for {}",
                    algorithm, distfile
                );
                return Ok(1);
            }
        }
        Ok(0)
    }
}
