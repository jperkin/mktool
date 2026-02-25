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
use elf::ElfBytes;
use elf::endian::AnyEndian;
use rayon::prelude::*;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

struct CTFFile {
    path: PathBuf,
    error: String,
}

#[derive(Args, Debug)]
pub struct CTFConvert {
    #[arg(short = 'c', value_name = "ctfconvert")]
    #[arg(help = "Path to ctfconvert command")]
    ctfconvert: PathBuf,

    #[arg(short = 'I', value_name = "input")]
    #[arg(help = "Read files from input file (\"-\" for stdin)")]
    input: Option<PathBuf>,

    #[arg(short = 'j', value_name = "jobs")]
    #[arg(help = "Maximum number of threads (or \"MKTOOL_JOBS\" env var)")]
    jobs: Option<usize>,

    #[arg(short = 's', value_name = "prefix")]
    #[arg(help = "Prefix to strip from output")]
    strip_prefix: PathBuf,
}

impl CTFConvert {
    pub fn run(&self) -> Result<i32, Box<dyn std::error::Error>> {
        /*
         * Add files passed in via -I (supporting stdin if set to "-").
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
                let file = PathBuf::from(line);
                if file.exists() {
                    inputfiles.insert(file);
                }
            }
        }

        /*
         * No input files, return early.
         */
        if inputfiles.is_empty() {
            return Ok(0);
        }

        /*
         * Create Vec of CTFFiles for parallel processing.
         */
        let mut inputfiles: Vec<CTFFile> = inputfiles
            .into_iter()
            .map(|path| CTFFile { path, error: String::new() })
            .collect();

        /*
         * Set up rayon threadpool.  -j argument has highest precedence, then
         * MKTOOLS_JOBS environment variable, finally MKTOOL_DEFAULT_THREADS.
         */
        let nthreads = match self.jobs {
            Some(n) => n,
            None => match env::var("MKTOOL_JOBS") {
                Ok(n) => match n.parse::<usize>() {
                    Ok(n) => n,
                    Err(e) => {
                        eprintln!(
                            "WARNING: invalid MKTOOL_JOBS '{n}': {e}, using default"
                        );
                        MKTOOL_DEFAULT_THREADS
                    }
                },
                Err(_) => MKTOOL_DEFAULT_THREADS,
            },
        };
        rayon::ThreadPoolBuilder::new().num_threads(nthreads).build_global()?;

        /*
         * The behaviour of ctfconvert with the "-m" flag that we use is as
         * follows:
         *
         *  - A successful conversion exits 0 with an output file but no
         *    stdout.
         *
         *  - A successful conversion with some issues exits 0 with warnings
         *    on stderr (for example "WARNING: file putenv.c is missing debug
         *    information")
         *
         *  - A failed conversion of a file that already contains CTF data
         *    exits 0 with no output and no output file.
         *
         *  - A failed conversion of a file that does not contain debug data
         *    exits 0 with no output and no output file.
         *
         *  - Attempting to convert a non-binary file (e.g. a shell script)
         *    exits 1 with some stderr.
         *
         * Thus we do the following:
         *
         *   - First check that the file is valid ELF, and skip all others.
         *
         *   - Send all stderr from ctfconvert back to stderr, with the
         *     filename added as a prefix.
         *
         *   - Ignore the exit status of ctfconvert, just test for the
         *     presence of an output file.
         *
         *   - If the output file contains .SUNW_ctf then print it to stdout
         *     to indicate a successful conversion, otherwise record an error.
         */
        inputfiles.par_iter_mut().for_each(|file| {
            let infile = match fs::read(&file.path) {
                Ok(f) => f,
                Err(e) => {
                    file.error = format!("unable to read file: {e}");
                    return;
                }
            };
            if ElfBytes::<AnyEndian>::minimal_parse(&infile).is_err() {
                return;
            }

            let mut outfile = PathBuf::from(&file.path);
            if let Some(fname) = outfile.file_name() {
                let mut newname = fname.to_os_string();
                newname.push(".ctf");
                outfile.set_file_name(newname);
            }

            let cmd = match Command::new(&self.ctfconvert)
                .arg("-m")
                .arg("-o")
                .arg(&outfile)
                .arg(&file.path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    file.error = format!("unable to execute ctfconvert: {e}");
                    return;
                }
            };
            let cmd = match cmd.wait_with_output() {
                Ok(c) => c,
                Err(e) => {
                    file.error = format!("ctfconvert failed: {e}");
                    let _ = fs::remove_file(&outfile);
                    return;
                }
            };

            /*
             * The input files are usually ${DESTDIR}${PREFIX}/... and so the
             * -s flag allows that prefix to be pruned for cleaner output.
             */
            let filename = match &file.path.strip_prefix(&self.strip_prefix) {
                Ok(s) => s.display(),
                Err(_) => file.path.display(),
            };

            /*
             * Print conversion errors to stderr, prefixed with the file
             * that caused them for easier diagnosis.
             */
            let stderr = String::from_utf8_lossy(&cmd.stderr);
            for line in stderr.lines() {
                eprintln!("{filename}: {line}");
            }

            if outfile.exists() {
                let out = match fs::read(&outfile) {
                    Ok(f) => f,
                    Err(e) => {
                        file.error = format!("unable to read output: {e}");
                        let _ = fs::remove_file(&outfile);
                        return;
                    }
                };
                let elf = match ElfBytes::<AnyEndian>::minimal_parse(
                    out.as_slice(),
                ) {
                    Ok(e) => e,
                    Err(e) => {
                        file.error = format!("output is not valid ELF: {e}");
                        let _ = fs::remove_file(&outfile);
                        return;
                    }
                };
                match elf.section_header_by_name(".SUNW_ctf") {
                    Ok(Some(_)) => {
                        println!("{filename}");
                        if let Err(e) = fs::rename(&outfile, &file.path) {
                            file.error =
                                format!("unable to rename output: {e}");
                            let _ = fs::remove_file(&outfile);
                        }
                    }
                    Ok(None) => {
                        file.error =
                            "output does not contain CTF data".to_string();
                        let _ = fs::remove_file(&outfile);
                    }
                    Err(e) => {
                        file.error =
                            format!("unable to parse ELF sections: {e}");
                        let _ = fs::remove_file(&outfile);
                    }
                }
            }
        });

        let mut rv = 0;
        for file in &inputfiles {
            if !file.error.is_empty() {
                eprintln!("{}: {}", file.path.display(), file.error);
                rv = 1;
            }
        }

        Ok(rv)
    }
}
