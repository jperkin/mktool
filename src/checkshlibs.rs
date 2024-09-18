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
use goblin::mach::Mach;
use std::env;
use std::fs;
use std::io::{self, BufRead};
use std::path::Path;

#[derive(Args, Debug)]
pub struct CheckShlibs {
    #[arg(short = 'j', value_name = "jobs")]
    #[arg(help = "Maximum number of threads (or \"MKTOOL_JOBS\" env var)")]
    jobs: Option<usize>,
}

impl CheckShlibs {
    /**
     * Verify a library dependency for an object.  Print any errors to stdout,
     * as that is the way this has been designed to work for some reason.
     */
    fn verify_lib(&self, obj: &Path, lib: &str) {
        /* "self" is seen as the first entry on macOS. */
        #[cfg(target_os = "macos")]
        if lib == "self" {
            return;
        }

        /*
         * Skip system libraries if requested on newer macOS.  Apple no
         * longer ship the actual file system entries so we can't test for
         * them in any reasonable way.
         */
        #[cfg(target_os = "macos")]
        if env::var("SKIP_SYSTEM_LIBS").is_ok()
            && (lib.starts_with("/System/Library")
                || lib.starts_with("/usr/lib"))
        {
            return;
        }

        /*
         * Library paths must not start with WRKDIR.
         */
        if let Ok(wrkdir) = env::var("WRKDIR") {
            if lib.starts_with(&wrkdir) {
                println!("{}: path relative to WRKDIR: {}", obj.display(), lib);
            }
        }

        /*
         * Library paths must be absolute.
         */
        if !lib.starts_with("/") {
            println!("{}: relative library path: {}", obj.display(), lib);
        }

        /*
         * Library paths must exist.
         */
        if !Path::new(lib).exists() {
            println!("{}: missing library: {}", obj.display(), lib);
        }
    }

    pub fn run(&self) -> Result<i32, Box<dyn std::error::Error>> {
        for line in io::stdin().lock().lines() {
            let line = line?;
            let path = Path::new(&line);
            let file = fs::read(path)?;
            let m = Mach::parse(&file).unwrap_or_else(|err| {
                eprintln!("ERROR: Unable to parse {}: {err}", path.display());
                std::process::exit(1);
            });

            let obj = match m {
                Mach::Fat(fat) => {
                    let ojnk = fat.get(0);
                    if let Ok(goblin::mach::SingleArch::MachO(o)) = ojnk {
                        o
                    } else {
                        eprintln!("ERROR: {line} is unsupported");
                        std::process::exit(1);
                    }
                }
                Mach::Binary(bin) => bin,
            };
            for lib in &obj.libs {
                self.verify_lib(path, lib);
            }
        }

        Ok(0)
    }
}
