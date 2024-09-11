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
use pkgsrc::digest::Digest;
use pkgsrc::distinfo::{Checksum, Distinfo, Entry, EntryType};
use rayon::prelude::*;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::str::FromStr;

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

    #[arg(short = 'j', value_name = "jobs")]
    #[arg(help = "Maximum number of threads (or \"MKTOOL_JOBS\" env var)")]
    jobs: Option<usize>,

    #[arg(short = 'p', value_name = "algorithm")]
    #[arg(help = "Algorithm digests to create for each patchfile")]
    palgorithms: Vec<String>,

    #[arg(value_name = "patch")]
    #[arg(help = "Alphabetical list of named patch files")]
    patchfiles: Vec<PathBuf>,
}

impl DistInfo {
    pub fn run(&self) -> Result<i32, Box<dyn std::error::Error>> {
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
         * Read existing distinfo file if specified.
         */
        let mut di_cur = Distinfo::new();
        if let Some(di) = &self.distinfo {
            /* If distinfo was specified then it must exist. */
            match fs::read(di) {
                Ok(s) => di_cur = Distinfo::from_bytes(&s),
                Err(e) => {
                    eprintln!(
                        "ERROR: Could not open distinfo '{}': {}",
                        di.display(),
                        e
                    );
                    return Ok(128);
                }
            }
        }

        /*
         * Distfiles can be passed using -I or -c, so first add them all to a
         * HashSet to ensure unique entries, and then later create each Entry.
         *
         * Only add distfiles that exist, and silently skip those that don't,
         * to match distinfo.awk behaviour.
         */
        let mut distfiles: HashSet<PathBuf> = HashSet::new();
        let mut entries: Vec<Entry> = vec![];

        /*
         * Add files specified by -c.
         */
        for file in &self.cksumfile {
            let mut fullpath = PathBuf::from(&self.distdir);
            fullpath.push(file);
            if fullpath.exists() {
                distfiles.insert(file.into());
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
                let file = line?;
                let mut fullpath = PathBuf::from(&self.distdir);
                fullpath.push(&file);
                if fullpath.exists() {
                    distfiles.insert(file.into());
                }
            }
        }

        /*
         * Add Entry for each unique distfile passed.
         */
        let mut distsums: Vec<Checksum> = vec![];
        for algorithm in &self.dalgorithms {
            let digest = Digest::from_str(algorithm)?;
            distsums.push(Checksum::new(digest, String::new()));
        }
        for distfile in distfiles {
            let mut fullpath = PathBuf::from(&self.distdir);
            fullpath.push(&distfile);
            let entry = Entry::new(distfile, fullpath, distsums.clone(), None);
            entries.push(entry);
        }

        /*
         * Add patchfiles added as command line arguments.  We may be passed
         * globs, so check the file actually exists first.
         */
        let mut patchsums: Vec<Checksum> = vec![];
        for algorithm in &self.palgorithms {
            let digest = Digest::from_str(algorithm)?;
            patchsums.push(Checksum::new(digest, String::new()));
        }
        for path in &self.patchfiles {
            if path.exists() {
                if let Some(filename) = path.file_name() {
                    let entry = Entry::new(
                        PathBuf::from(filename),
                        path.to_path_buf(),
                        patchsums.clone(),
                        None,
                    );
                    entries.push(entry);
                }
            }
        }

        /*
         * Special case to match distinfo.awk behaviour.  If we were passed a
         * valid distinfo file but no distfiles, then exit 1 with no output.
         * If no distinfo then we continue so that an empty entry is printed.
         *
         * Note that we specifically need to ensure patchfiles is empty too so
         * that makepatchsum (which passes a patch-* glob) with no patch files
         * that exist still prints a valid distinfo.  Save noinputfiles for
         * later use.
         */
        let noinputfiles = entries.is_empty() && self.patchfiles.is_empty();
        if noinputfiles && self.distinfo.is_some() {
            return Ok(1);
        }

        /*
         * Order all of the input files alphabetically.  Distfiles and
         * patchfiles are separated in the final output by Distinfo.
         */
        entries.sort_by(|a, b| a.filepath.cmp(&b.filepath));

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
         * Calculate checksums for each Entry, and size for Distfile entries,
         * storing results back into the Entry.
         */
        entries.par_iter_mut().for_each(|entry| {
            for c in entry.checksums.iter_mut() {
                match Distinfo::calculate_checksum(&entry.filepath, c.digest) {
                    Ok(h) => c.hash = h,
                    Err(e) => {
                        eprintln!(
                            "Unable to calculate checksum for {}: {}",
                            &entry.filepath.display(),
                            e
                        );
                    }
                };
            }
            if entry.filetype == EntryType::Distfile {
                match Distinfo::calculate_size(&entry.filepath) {
                    Ok(s) => entry.size = Some(s),
                    Err(e) => {
                        eprintln!(
                            "Unable to calculate size for {}: {}",
                            &entry.filepath.display(),
                            e
                        );
                    }
                };
            }
        });

        /*
         * We have all the data we need.  Start constructing our new Distinfo.
         */
        let mut di_new = Distinfo::new();

        if let Some(rcsid) = di_cur.rcsid() {
            di_new.set_rcsid(rcsid);
        }
        for entry in &entries {
            di_new.insert(entry.clone());
        }

        /*
         * If we weren't passed any distfiles, but there are entries in an
         * existing distinfo, then they need to be retained.  This is how
         * "makepatchsum" operates, by just operating on patch files and
         * keeping any file entries.
         */
        if di_new.distfiles().is_empty() {
            for distfile in di_cur.distfiles() {
                di_new.insert(distfile.clone());
            }
        }

        /*
         * If we weren't passed any patchfiles, but there are entries in an
         * existing distinfo, then they need to be retained.  This is how
         * "makesum" operates, by just operating on distfiles and
         * keeping any patch entries.
         */
        if di_new.patchfiles().is_empty() {
            for patchfile in di_cur.patchfiles() {
                di_new.insert(patchfile.clone());
            }
        }

        /*
         * Write resulting distinfo file to stdout.
         */
        let mut stdout = io::stdout().lock();
        stdout.write_all(&di_new.as_bytes())?;
        stdout.flush()?;

        /*
         * Special case for no input files at all, otherwise return based on
         * whether the new contents match the old.
         */
        if noinputfiles {
            Ok(1)
        } else {
            Ok((di_cur.as_bytes() != di_new.as_bytes()) as i32)
        }
    }
}
