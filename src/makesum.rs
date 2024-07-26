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

use blake2::{Blake2s256, Digest};
use clap::Args;
use ripemd::Ripemd160;
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use std::fmt::Write;
use std::fs;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

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

impl MakeSum {
    pub fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        /*
         * Create list of distfiles.  In reality the user will choose either
         * the -c option to specify each individually, or -I to read from a
         * file, but we support both simultaneously because why not.
         */
        let mut distfiles: Vec<PathBuf> = vec![];
        for f in &self.cksumfile {
            /*
             * Only add distfiles that exist, and silently skip those that
             * don't, to match distinfo.awk behaviour.
             */
            let mut d = PathBuf::from(&self.distdir);
            d.push(f);
            if d.exists() {
                distfiles.push(d);
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
                        distfiles.push(d);
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
                        distfiles.push(d);
                    }
                }
            }
        }
        for d in &distfiles {
            if !d.exists() {
                continue;
            }
            for a in &self.dalgorithms {
                let mut file = fs::File::open(d)?;
                let h = match a.as_str() {
                    "BLAKE2s" => self.hash_file::<Blake2s256>(&mut file),
                    "RMD160" => self.hash_file::<Ripemd160>(&mut file),
                    "SHA1" => self.hash_file::<Sha1>(&mut file),
                    "SHA256" => self.hash_file::<Sha256>(&mut file),
                    "SHA512" => self.hash_file::<Sha512>(&mut file),
                    _ => unimplemented!("unsupported algorithm: {}", a),
                };
                println!(
                    "{} ({}) = {}",
                    a,
                    d.strip_prefix(&self.distdir)?.display(),
                    h
                );
            }
        }
        for p in &self.patchfiles {
            if !p.exists() {
                continue;
            }
            for a in &self.palgorithms {
                let mut file = fs::File::open(p)?;
                let h = match a.as_str() {
                    "BLAKE2s" => self.hash_patch::<Blake2s256>(&mut file),
                    "RMD160" => self.hash_patch::<Ripemd160>(&mut file),
                    "SHA1" => self.hash_patch::<Sha1>(&mut file),
                    "SHA256" => self.hash_patch::<Sha256>(&mut file),
                    "SHA512" => self.hash_patch::<Sha512>(&mut file),
                    _ => unimplemented!("unsupported algorithm: {}", a),
                };
                println!(
                    "{} ({}) = {}",
                    a,
                    p.file_name().expect("").to_str().expect(""),
                    h?
                );
            }
        }
        Ok(())
    }

    fn hash_file<D: Digest + std::io::Write>(&self, file: &mut File) -> String {
        let mut hasher = D::new();
        let _ = io::copy(file, &mut hasher);
        hasher
            .finalize()
            .iter()
            .fold(String::new(), |mut output, b| {
                let _ = write!(output, "{b:02x}");
                output
            })
    }

    fn hash_patch<D: Digest + std::io::Write>(
        &self,
        file: &mut File,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut hasher = D::new();

        let mut r = BufReader::new(file);
        let mut s = String::new();
        r.read_to_string(&mut s)?;

        for line in s.split_inclusive('\n') {
            if line.contains("$NetBSD") {
                continue;
            }
            hasher.update(line.as_bytes());
        }
        let hash = hasher.finalize();
        Ok(hash.iter().fold(String::new(), |mut output, b| {
            let _ = write!(output, "{b:02x}");
            output
        }))
    }
}
