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
use rayon::prelude::*;
use std::env;
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Args, Debug)]
pub struct DigestCmd {
    #[arg(value_name = "algorithm")]
    #[arg(help = "Algorithm to use")]
    algorithm: String,

    #[arg(short = 'j', value_name = "jobs")]
    #[arg(help = "Maximum number of threads (or \"MKTOOL_JOBS\" env var)")]
    jobs: Option<usize>,

    #[arg(value_name = "file")]
    #[arg(help = "List of files to calculate checksums for")]
    files: Option<Vec<PathBuf>>,
}

struct DigestResult {
    path: PathBuf,
    hash: Option<String>,
    error: String,
}

impl DigestCmd {
    pub fn run(&self) -> Result<i32, Box<dyn std::error::Error>> {
        let algorithm = Digest::from_str(&self.algorithm)?;

        /*
         * If we're passed any input files then set them up for parallel
         * processing, otherwise we operate in stdin mode: just perform the
         * calculation immediately and return.
         */
        let Some(files) = &self.files else {
            let mut input = Vec::new();
            io::stdin().read_to_end(&mut input)?;
            let mut cursor = Cursor::new(input);
            println!("{}", algorithm.hash_file(&mut cursor)?);
            return Ok(0);
        };

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
        rayon::ThreadPoolBuilder::new()
            .num_threads(nthreads)
            .build_global()
            .unwrap();

        /*
         * Set up a vec of DigestResult so that the calculated hashes can be
         * stored before printing the results in the same order they were given.
         */
        let mut hashfiles: Vec<DigestResult> = files
            .iter()
            .map(|f| DigestResult {
                path: f.to_path_buf(),
                hash: None,
                error: String::new(),
            })
            .collect();

        hashfiles.par_iter_mut().for_each(|file| {
            match fs::File::open(&file.path) {
                Ok(mut f) => match algorithm.hash_file(&mut f) {
                    Ok(h) => file.hash = Some(h),
                    Err(e) => file.error = e.to_string(),
                },
                Err(e) => file.error = e.to_string(),
            }
        });

        /*
         * If there are any missing hashes (e.g. a filename that does not
         * exist) then print them to stderr and set the exit status to 1,
         * mirroring the behaviour of pkgtools/digest.
         */
        let mut rv = 0;
        for file in hashfiles {
            if let Some(hash) = file.hash {
                println!("{} ({}) = {}", algorithm, file.path.display(), hash);
            } else {
                eprintln!("{}: {}", file.path.display(), file.error);
                rv = 1;
            }
        }

        Ok(rv)
    }
}
