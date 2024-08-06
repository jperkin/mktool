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
use std::fs;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Args, Debug)]
pub struct DistInfo {
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

    #[arg(short = 'j', value_name = "jobs", default_value = "4")]
    #[arg(help = "Number of parallel jobs to process at a time")]
    jobs: u64,

    #[arg(short = 'p', value_name = "algorithm")]
    #[arg(help = "Algorithm digests to create for each patchfile")]
    palgorithms: Vec<String>,

    #[arg(value_name = "patch")]
    #[arg(help = "Alphabetical list of named patch files")]
    patchfiles: Vec<PathBuf>,
}

/**
 * [`DistInfoType`] contains the type of file, as source files and patches are
 * handled differently.
 */
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub enum DistInfoType {
    /**
     * Regular distribution file, e.g. source tarball for a package.
     */
    #[default]
    Distfile,
    /**
     * A pkgsrc patch file that modifies the package source.
     */
    Patch,
}

/**
 * [`HashEntry`] contains the [`Digest`] type and the [`String`] hash it
 * calculated for an associated file.
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct HashEntry {
    /**
     * The [`Digest`] type used for this entry.
     */
    digest: Digest,
    /**
     * A [`String`] result after the digest hash has been calculated.
     */
    hash: String,
}

/**
 * [`DistInfoEntry`] contains information about a file entry in the distinfo file.
 */
#[derive(Clone, Debug, Default)]
pub struct DistInfoEntry {
    /**
     * Whether this is a distfile or a patch file.
     */
    pub filetype: DistInfoType,
    /**
     * Full path to file.
     */
    pub filepath: PathBuf,
    /**
     * Filename for printing.  Must be valid UTF-8.
     */
    pub filename: String,
    /**
     * File size (not used for patches).
     */
    pub size: u64,
    /**
     * Computed hashes, one entry per Digest type.
     */
    pub hashes: Vec<HashEntry>,
    /**
     * Whether this entry has been processed.  What that means in practise
     * will differ depending on users of this struct.
     */
    pub processed: bool,
}

impl DistInfoEntry {
    fn calculate(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for h in &mut self.hashes {
            let mut f = fs::File::open(&self.filepath)?;
            match self.filetype {
                DistInfoType::Distfile => {
                    h.hash = h.digest.hash_file(&mut f)?;
                }
                DistInfoType::Patch => {
                    h.hash = h.digest.hash_patch(&mut f)?;
                }
            };
        }
        if self.filetype == DistInfoType::Distfile {
            let f = fs::File::open(&self.filepath)?;
            let m = f.metadata()?;
            self.size = m.len();
        }
        Ok(())
    }
}

impl DistInfo {
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
         * On the first pass all of the supplied files (whether distfiles or
         * patchfiles) are added to a Vec of DistInfoEntry's, along with the
         * algorithms to be calculated for each.
         */
        let mut distinfo: Vec<DistInfoEntry> = vec![];

        /*
         * Create a hashes vec that we can clone for each distinfo entry.
         */
        let mut d_hashes: Vec<HashEntry> = vec![];
        for a in &self.dalgorithms {
            let d = Digest::from_str(a)?;
            let he = HashEntry {
                digest: d,
                hash: String::new(),
            };
            d_hashes.push(he);
        }

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
                let f = d.clone();
                let f = match f.strip_prefix(&self.distdir)?.to_str() {
                    Some(s) => s,
                    None => {
                        eprintln!(
                            "ERROR: File '{}' is not valid unicode.",
                            d.display()
                        );
                        std::process::exit(1);
                    }
                };
                let n = DistInfoEntry {
                    filetype: DistInfoType::Distfile,
                    filepath: d,
                    filename: f.to_string(),
                    hashes: d_hashes.clone(),
                    ..Default::default()
                };
                distinfo.push(n);
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
                    let f = d.clone();
                    let f = match f.strip_prefix(&self.distdir)?.to_str() {
                        Some(s) => s,
                        None => {
                            eprintln!(
                                "ERROR: File '{}' is not valid unicode.",
                                d.display()
                            );
                            std::process::exit(1);
                        }
                    };
                    let n = DistInfoEntry {
                        filetype: DistInfoType::Distfile,
                        filepath: d,
                        filename: f.to_string(),
                        hashes: d_hashes.clone(),
                        ..Default::default()
                    };
                    distinfo.push(n);
                }
            }
        }

        /*
         * If we weren't passed any distfiles, but there are entries in an
         * existing distinfo, then they need to be retained.  This is how
         * "makepatchsum" operates, by just operating on patch files and
         * keeping any file entries.
         */
        if distinfo.is_empty() {
            for (i, line) in input.lines().enumerate() {
                // Skip $NetBSD$ and blank line
                if i < 2 {
                    continue;
                }
                let line = line?;
                let s = line.split(&['(', ')']).collect::<Vec<_>>();
                if s.len() == 3 && is_patchfile(s[1]).is_none() {
                    output.extend_from_slice(line.as_bytes());
                    output.extend_from_slice("\n".as_bytes());
                }
            }
        }

        /*
         * Create a hashes Vec based on the requested algorithms from the
         * command line argument that we can clone for each patchfile entry.
         */
        let mut p_hashes: Vec<HashEntry> = vec![];
        for a in &self.palgorithms {
            let d = Digest::from_str(a)?;
            let he = HashEntry {
                digest: d,
                hash: String::new(),
            };
            p_hashes.push(he);
        }

        /*
         * Add patchfiles added as command line arguments.
         */
        for path in &self.patchfiles {
            if let Some(filename) = is_patchpath(path) {
                let n = DistInfoEntry {
                    filetype: DistInfoType::Patch,
                    filepath: path.to_path_buf(),
                    filename,
                    hashes: p_hashes.clone(),
                    ..Default::default()
                };
                distinfo.push(n);
            }
        }

        /*
         * Order the Vec so that distfiles come first, followed by patches,
         * and hashes for each are ordered by how they were specified on the
         * command line.
         */
        distinfo.sort_by(|a, b| a.filepath.cmp(&b.filepath));

        /*
         * Now set up parallel processing of the Vec.
         */
        let mut threads = vec![];
        let active_threads = Arc::new(Mutex::new(0));
        let max_threads = self.jobs;

        /*
         * Wrap each distinfo entry in its own Mutex.
         */
        let distinfo: Vec<_> = distinfo
            .into_iter()
            .map(|s| Arc::new(Mutex::new(s)))
            .collect();

        /*
         * Each active thread calls the .calculate() function for its entry
         * in the Vec, which stores the resulting hashes (and optional size)
         * back into the entry.
         */
        for d in &distinfo {
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
         * Now that we're done processing, unwrap back to a plain Vec for
         * simpler access.
         */
        let distinfo: Vec<_> = distinfo
            .into_iter()
            .map(|arc_mutex| {
                Arc::try_unwrap(arc_mutex)
                    .ok()
                    .unwrap()
                    .into_inner()
                    .unwrap()
            })
            .collect();

        /*
         * Write distfiles.
         */
        for distfile in distinfo
            .iter()
            .filter(|&d| d.filetype == DistInfoType::Distfile)
        {
            let f = distfile.filepath.strip_prefix(&self.distdir)?;
            for h in &distfile.hashes {
                output.extend_from_slice(
                    format!("{} ({}) = {}\n", h.digest, f.display(), h.hash,)
                        .as_bytes(),
                );
            }
            output.extend_from_slice(
                format!("Size ({}) = {} bytes\n", f.display(), distfile.size)
                    .as_bytes(),
            );
        }

        /*
         * Write patches.
         */
        for patchfile in distinfo
            .iter()
            .filter(|&d| d.filetype == DistInfoType::Patch)
        {
            for h in &patchfile.hashes {
                output.extend_from_slice(
                    format!(
                        "{} ({}) = {}\n",
                        h.digest, patchfile.filename, h.hash,
                    )
                    .as_bytes(),
                );
            }
        }

        /*
         * If we weren't passed any patchfiles, but there are entries in an
         * existing distinfo, then they need to be retained.  This is how
         * "makesum" operates, by just operating on distfiles and
         * keeping any patch entries.
         */
        if self.patchfiles.is_empty() {
            for (i, line) in input.lines().enumerate() {
                // Skip $NetBSD$ and blank line
                if i < 2 {
                    continue;
                }
                let line = line?;
                let s = line.split(&['(', ')']).collect::<Vec<_>>();
                if s.len() == 3 && is_patchfile(s[1]).is_some() {
                    output.extend_from_slice(line.as_bytes());
                    output.extend_from_slice("\n".as_bytes());
                }
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
fn is_patchfile(s: &str) -> Option<String> {
    /*
     * Skip local patches or temporary patch files created by e.g. mkpatches.
     */
    if s.starts_with("patch-local-")
        || s.ends_with(".orig")
        || s.ends_with(".rej")
        || s.ends_with("~")
    {
        return None;
    }
    /*
     * Match valid patch filenames.
     */
    if s.starts_with("patch-")
        || (s.starts_with("emul-") && s.contains("-patch-"))
    {
        return Some(s.to_string());
    }

    /*
     * Anything else is invalid.
     */
    None
}

fn is_patchpath(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    is_patchfile(path.file_name()?.to_str()?)
}
