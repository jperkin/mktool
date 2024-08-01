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
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Args, Debug)]
pub struct MakeSum {
    #[arg(short = 'a', value_name = "algorithm")]
    #[arg(help = "Algorithm digests to create for each distfile")]
    dalgorithms: Vec<String>,

    #[arg(short, value_name = "distfile")]
    #[arg(help = "Generate digest for each named distfile")]
    cksumfile: Vec<PathBuf>,

    #[arg(short, value_name = "distdir", default_value = ".")]
    #[arg(help = "Directory under which distfiles are found")]
    distdir: PathBuf,

    #[arg(short = 'f', value_name = "distinfo")]
    #[arg(help = "Path to an existing distinfo file")]
    distinfo: Option<PathBuf>,

    #[arg(short = 'I', value_name = "input")]
    #[arg(help = "Read distfiles from input instead of -c")]
    input: Option<PathBuf>,

    #[arg(short, value_name = "ignorefile")]
    #[arg(help = "List of distfiles to ignore (unused)")]
    ignorefile: Option<PathBuf>,

    #[arg(short = 'p', value_name = "algorithm")]
    #[arg(help = "Algorithm digests to create for each patchfile")]
    palgorithms: Vec<String>,

    #[arg(value_name = "patch")]
    #[arg(help = "Alphabetical list of named patch files")]
    patchfiles: Vec<PathBuf>,
}

#[derive(Debug, Default)]
struct SumResult {
    filename: PathBuf,
    size: u64,
    hashes: HashMap<Digest, String>,
}

impl MakeSum {
    pub fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        /*
         * Create list of distfiles.  In reality the user will choose either
         * the -c option to specify each individually, or -I to read from a
         * file, but we support both simultaneously because why not.
         */
        let mut distfiles: Vec<SumResult> = vec![];
        for f in &self.cksumfile {
            /*
             * Only add distfiles that exist, and silently skip those that
             * don't, to match distinfo.awk behaviour.
             */
            let mut d = PathBuf::from(&self.distdir);
            d.push(f);
            if d.exists() {
                let n = SumResult {
                    filename: d,
                    ..Default::default()
                };
                distfiles.push(n);
            }
        }
        if let Some(infile) = &self.input {
            if infile == Path::new("-") {
                let f = io::stdin();
                let r = f.lock();
                for line in r.lines() {
                    let line = line?;
                    let mut d = PathBuf::from(&self.distdir);
                    d.push(line);
                    if d.exists() {
                        let n = SumResult {
                            filename: d,
                            ..Default::default()
                        };
                        distfiles.push(n);
                    }
                }
            } else {
                let f = File::open(infile)?;
                let r = BufReader::new(f);
                for line in r.lines() {
                    let line = line?;
                    let mut d = PathBuf::from(&self.distdir);
                    d.push(line);
                    if d.exists() {
                        let n = SumResult {
                            filename: d,
                            ..Default::default()
                        };
                        distfiles.push(n);
                    }
                }
            }
        }
        for d in &mut distfiles {
            for a in &self.dalgorithms {
                let mut file = fs::File::open(&d.filename)?;
                let alg = Digest::from_str(a)?;
                let hash = alg.hash_file(&mut file)?;
                d.hashes.insert(alg, hash);
            }
            let file = fs::File::open(&d.filename)?;
            let m = file.metadata()?;
            d.size = m.len();
            dbg!(&d);
        }
        for d in distfiles {
            let filename = d.filename.strip_prefix(&self.distdir)?;
            for a in &self.dalgorithms {
                let alg = Digest::from_str(a)?;
                println!(
                    "{} ({}) = {}",
                    a,
                    filename.display(),
                    d.hashes.get(&alg).unwrap()
                );
            }
            println!("Size ({}) = {} bytes", filename.display(), d.size);
        }

        for p in &self.patchfiles {
            if !p.exists() {
                continue;
            }
            for a in &self.palgorithms {
                let mut file = fs::File::open(p)?;
                let d = Digest::from_str(a)?;
                let h = d.hash_patch(&mut file)?;
                println!(
                    "{} ({}) = {}",
                    a,
                    p.file_name().expect("").to_str().expect(""),
                    h
                );
            }
        }
        Ok(())
    }
}
