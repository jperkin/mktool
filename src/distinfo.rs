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
use pkgsrc::distinfo::{Checksum, Distinfo, Entry};
use std::env;
use std::fs;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::ffi::OsStrExt;
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

    #[arg(short = 'j', value_name = "jobs")]
    #[arg(help = "Maximum number of threads (or \"MKTOOL_JOBS\" env var)")]
    jobs: Option<u64>,

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
 * [`Distfile`] contains information about a file entry in the distinfo file.
 */
#[derive(Clone, Debug, Default)]
pub struct Distfile {
    /**
     * Full path to file.
     */
    pub filepath: PathBuf,
    /**
     * Filename entry in the distinfo file.  Note that this may be different
     * to that used in [`filepath`], as there is support for strip-suffix mode
     * where e.g. foo.tar.gz.download can be compared against foo.tar.gz.
     */
    pub filename: PathBuf,
    /**
     * Whether this is a distfile or a patch file.
     */
    pub filetype: DistInfoType,
    /**
     * Information about this distfile (checksums and size) stored in a
     * Distinfo [`Entry`].
     */
    pub entry: Entry,
    /**
     * Whether this entry has been processed.  What that means in practise
     * will differ depending on users of this struct.
     */
    pub processed: bool,
}

impl Distfile {
    pub fn calculate(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for h in &mut self.entry.checksums {
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
            self.entry.size = Some(m.len());
        }
        Ok(())
    }
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
        let mut distinfo = Distinfo::new();
        if let Some(di) = &self.distinfo {
            /* If distinfo was specified then it must exist. */
            match fs::read(di) {
                Ok(s) => distinfo = Distinfo::from_bytes(&s),
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
         * Create hashes vecs that we can clone for each distfile or patchfile.
         */
        let mut d_hashes: Vec<Checksum> = vec![];
        for a in &self.dalgorithms {
            let d = Digest::from_str(a)?;
            let he = Checksum {
                digest: d,
                hash: String::new(),
            };
            d_hashes.push(he);
        }
        let mut p_hashes: Vec<Checksum> = vec![];
        for a in &self.palgorithms {
            let d = Digest::from_str(a)?;
            let he = Checksum {
                digest: d,
                hash: String::new(),
            };
            p_hashes.push(he);
        }

        /*
         * Collect all of the input files and what algorithms are to be
         * computed for them.
         */
        let mut inputfiles: Vec<Distfile> = vec![];

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
                let e = Entry {
                    filename: f.into(),
                    checksums: d_hashes.clone(),
                    ..Default::default()
                };
                let n = Distfile {
                    filetype: DistInfoType::Distfile,
                    filepath: d,
                    entry: e,
                    ..Default::default()
                };
                inputfiles.push(n);
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
                    let e = Entry {
                        filename: f.into(),
                        checksums: d_hashes.clone(),
                        ..Default::default()
                    };
                    let n = Distfile {
                        filetype: DistInfoType::Distfile,
                        filepath: d,
                        entry: e,
                        ..Default::default()
                    };
                    inputfiles.push(n);
                }
            }
        }

        /*
         * Add patchfiles added as command line arguments.
         */
        for path in &self.patchfiles {
            if let Some(filename) = is_patchpath(path) {
                let e = Entry {
                    filename: filename.into(),
                    checksums: p_hashes.clone(),
                    ..Default::default()
                };
                let n = Distfile {
                    filetype: DistInfoType::Patch,
                    filepath: path.to_path_buf(),
                    entry: e,
                    ..Default::default()
                };
                inputfiles.push(n);
            }
        }

        /*
         * Order all of the input files alphabetically.  Distfiles and
         * patchfiles will be separated later.
         */
        inputfiles.sort_by(|a, b| a.filepath.cmp(&b.filepath));

        /*
         * Now set up parallel processing of the Vec.
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

        /*
         * Wrap each distfile entry in its own Mutex.
         */
        let inputfiles: Vec<_> = inputfiles
            .into_iter()
            .map(|s| Arc::new(Mutex::new(s)))
            .collect();

        /*
         * Each active thread calls the .calculate() function for its entry
         * in the Vec, which stores the resulting hashes (and optional size)
         * back into the entry.
         */
        for d in &inputfiles {
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
         * simpler access, and set up separate distfile and patchfile vecs.
         */
        let inputfiles: Vec<_> = inputfiles
            .into_iter()
            .map(|arc_mutex| {
                Arc::try_unwrap(arc_mutex)
                    .ok()
                    .unwrap()
                    .into_inner()
                    .unwrap()
            })
            .collect();
        let distfiles: Vec<_> = inputfiles
            .iter()
            .filter(|&d| d.filetype == DistInfoType::Distfile)
            .collect();
        let patchfiles: Vec<_> = inputfiles
            .iter()
            .filter(|&d| d.filetype == DistInfoType::Patch)
            .collect();

        /*
         * Special case to match distinfo.awk behaviour.  If we were passed a
         * valid distinfo file (already tested earlier) but no input files,
         * then exit 1 with no output.
         */
        if inputfiles.is_empty() && self.distinfo.is_some() {
            return Ok(1);
        }

        /*
         * We have all the data we need.  Start constructing our output.
         */
        let mut output: Vec<u8> = vec![];

        /*
         * Add $NetBSD$ header if there isn't one already, and blank line
         * before starting checksums.
         */
        match distinfo.rcsid() {
            Some(s) => output.extend_from_slice(s.as_bytes()),
            None => output.extend_from_slice("$NetBSD$".as_bytes()),
        };
        output.extend_from_slice("\n\n".as_bytes());

        /*
         * If we weren't passed any distfiles, but there are entries in an
         * existing distinfo, then they need to be retained.  This is how
         * "makepatchsum" operates, by just operating on patch files and
         * keeping any file entries.
         */
        if distfiles.is_empty() {
            for f in distinfo.files() {
                output.extend_from_slice(&f.as_bytes());
            }
        } else {
            for distfile in distfiles {
                let f = distfile.filepath.strip_prefix(&self.distdir)?;
                for h in &distfile.entry.checksums {
                    output.extend_from_slice(
                        format!(
                            "{} ({}) = {}\n",
                            h.digest,
                            f.display(),
                            h.hash,
                        )
                        .as_bytes(),
                    );
                }
                if let Some(size) = distfile.entry.size {
                    output.extend_from_slice(
                        format!("Size ({}) = {} bytes\n", f.display(), size)
                            .as_bytes(),
                    );
                }
            }
        }

        /*
         * If we weren't passed any patchfiles, but there are entries in an
         * existing distinfo, then they need to be retained.  This is how
         * "makesum" operates, by just operating on distfiles and
         * keeping any patch entries.
         */
        if patchfiles.is_empty() {
            for p in distinfo.patches() {
                output.extend_from_slice(&p.as_bytes());
            }
        } else {
            for patchfile in patchfiles {
                for h in &patchfile.entry.checksums {
                    output.extend_from_slice(
                        format!(
                            "{} ({}) = {}\n",
                            h.digest,
                            patchfile.entry.filename.display(),
                            h.hash,
                        )
                        .as_bytes(),
                    );
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
        if !inputfiles.is_empty() && distinfo.as_bytes() == output {
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
