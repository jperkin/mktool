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
use std::io::{self, BufRead, BufReader, Write};
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

/**
 * [`SumResult`] contains information about a file.
 */
#[derive(Debug, Default)]
struct SumResult {
    /**
     * Full path to input file.
     */
    filepath: PathBuf,
    /**
     * String containing filename portion of filepath.
     */
    filename: String,
    /**
     * Size of file (not used for patches).
     */
    size: u64,
    /**
     * Map of hashes.
     */
    hashes: HashMap<Digest, String>,
}

impl MakeSum {
    pub fn run(&self) -> Result<i32, Box<dyn std::error::Error>> {
        /*
         * Store input "distinfo" and output as u8 vecs.  These are compared at
         * the end to determine the exit status (0 if no change, 1 if new or
         * changed.
         */
        let mut input: Vec<u8> = vec![];
        let mut output: Vec<u8> = vec![];

        /*
         * Check for valid distdir, exit early with a compatible exit code if
         * not.
         */
        if !self.distdir.is_dir() {
            eprintln!(
                "ERROR: Supplied DISTDIR at '{}' is not a directory",
                self.distdir.display()
            );
            return Ok(128);
        }

        /*
         * Read existing distinfo file if specified.  Any $NetBSD$ header is
         * retained.
         */
        if let Some(di) = &self.distinfo {
            /* If distinfo was specified then it must exist. */
            match fs::read(di) {
                Ok(s) => input = s,
                Err(e) => {
                    eprintln!(
                        "ERROR: Could not open distinfo '{}': {}",
                        di.display(),
                        e
                    );
                    return Ok(128);
                }
            }
            if let Some(first) = input.lines().nth(0) {
                let first = first?;
                if first.starts_with("$NetBSD") {
                    output.extend_from_slice(first.as_bytes());
                    output.extend_from_slice("\n".as_bytes());
                }
            }
        }

        /*
         * Add $NetBSD$ header if there isn't one already, and blank line
         * before starting checksums.
         */
        if output.is_empty() {
            output.extend_from_slice("$NetBSD$\n".as_bytes());
        }
        output.extend_from_slice("\n".as_bytes());

        /*
         * Create list of distfiles.  In reality the user will choose either
         * the -c option to specify each individually, or -I to read from a
         * file, but we support both simultaneously because why not.
         */
        let mut distfiles: Vec<SumResult> = vec![];

        /*
         * Add files specified by -c.
         */
        for f in &self.cksumfile {
            /*
             * Only add distfiles that exist, and silently skip those that
             * don't, to match distinfo.awk behaviour.
             */
            let mut d = PathBuf::from(&self.distdir);
            d.push(f);
            if d.exists() {
                let n = SumResult {
                    filepath: d,
                    ..Default::default()
                };
                distfiles.push(n);
            }
        }

        /*
         * Add files passed in via -I (supporting stdin if set to "-").
         */
        if let Some(infile) = &self.input {
            let reader: Box<dyn io::BufRead> = match infile.to_str() {
                Some("-") => Box::new(io::stdin().lock()),
                Some(f) => Box::new(BufReader::new(File::open(f)?)),
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
                let mut d = PathBuf::from(&self.distdir);
                d.push(line);
                if d.exists() {
                    let n = SumResult {
                        filepath: d,
                        ..Default::default()
                    };
                    distfiles.push(n);
                }
            }
        }

        /*
         * Calculate hashes for each distfile.
         */
        for d in &mut distfiles {
            for a in &self.dalgorithms {
                let mut file = fs::File::open(&d.filepath)?;
                let alg = Digest::from_str(a)?;
                let hash = alg.hash_file(&mut file)?;
                d.hashes.insert(alg, hash);
            }
            let file = fs::File::open(&d.filepath)?;
            let m = file.metadata()?;
            d.size = m.len();
        }

        /* Distfiles are sorted by filename regardless of input order. */
        distfiles.sort_by(|a, b| a.filepath.cmp(&b.filepath));

        /*
         * Write each distfile to output.
         */
        for d in distfiles {
            let filename = d.filepath.strip_prefix(&self.distdir)?;
            for a in &self.dalgorithms {
                let alg = Digest::from_str(a)?;
                output.extend_from_slice(
                    format!(
                        "{} ({}) = {}\n",
                        a,
                        filename.display(),
                        d.hashes.get(&alg).unwrap()
                    )
                    .as_bytes(),
                );
            }
            output.extend_from_slice(
                format!("Size ({}) = {} bytes\n", filename.display(), d.size)
                    .as_bytes(),
            );
        }

        /*
         * Build list of patchfiles.
         */
        let mut patchfiles: Vec<SumResult> = vec![];
        for path in &self.patchfiles {
            if let Some(filename) = is_patchfile(path) {
                let n = SumResult {
                    filepath: path.to_path_buf(),
                    filename,
                    ..Default::default()
                };
                patchfiles.push(n);
            }
        }

        /*
         * Calculate hashes for each patchfile.
         */
        for p in &mut patchfiles {
            for a in &self.palgorithms {
                let mut file = fs::File::open(&p.filepath)?;
                let d = Digest::from_str(a)?;
                let h = d.hash_patch(&mut file)?;
                p.hashes.insert(d, h);
            }
        }

        /* Patches are sorted by filename regardless of input order. */
        patchfiles.sort_by(|a, b| a.filename.cmp(&b.filename));

        /*
         * Write each patchfile to output.
         */
        for p in patchfiles {
            for a in &self.palgorithms {
                let alg = Digest::from_str(a)?;
                output.extend_from_slice(
                    format!(
                        "{} ({}) = {}\n",
                        a,
                        p.filename,
                        p.hashes.get(&alg).unwrap()
                    )
                    .as_bytes(),
                );
            }
        }

        /*
         * Write resulting distinfo file to stdout.
         */
        let mut stdout = io::stdout().lock();
        stdout.write_all(&output)?;
        stdout.flush()?;

        /*
         * Return exit code based on whether there were changes or not.
         */
        if input == output {
            Ok(0)
        } else {
            Ok(1)
        }
    }
}

/*
 * Verify that a supplied path is a valid patch file.  Returns a String
 * containing the patch filename if so, otherwise None.
 */
fn is_patchfile(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }

    let patchfile = path.file_name()?.to_str()?;

    /*
     * Skip local patches or temporary patch files created by e.g. mkpatches.
     */
    if patchfile.starts_with("patch-local-")
        || patchfile.ends_with(".orig")
        || patchfile.ends_with(".rej")
        || patchfile.ends_with("~")
    {
        return None;
    }
    /*
     * Match valid patch filenames.
     */
    if patchfile.starts_with("patch-")
        || (patchfile.starts_with("emul-") && patchfile.contains("-patch-"))
    {
        return Some(patchfile.to_string());
    }

    /*
     * Anything else is invalid.
     */
    None
}
