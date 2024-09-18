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

use crate::check_shlibs::{CheckCache, CheckShlibs};
use goblin::elf::Elf;
use std::env;
use std::path::{Path, PathBuf};

impl CheckShlibs {
    pub fn check_dso(
        &self,
        path: &Path,
        object: &[u8],
        cache: &mut CheckCache,
    ) {
        let elf = match Elf::parse(object) {
            Ok(o) => o,
            Err(_) => return,
        };
        let runpath: Vec<String> = match elf.runpaths.first() {
            Some(p) => p.split(':').map(|s| s.to_string()).collect(),
            None => vec![],
        };
        let mut syspath: Vec<String> = vec![];
        if let Ok(paths) = env::var("PLATFORM_RPATH") {
            for path in paths.split(':').collect::<Vec<&str>>() {
                syspath.push(path.to_string());
            }
        }

        /*
         * With ELF we have a list of library requirements, and a list of paths
         * to search for them.  Try the paths from RUNPATH first, before
         * falling back to the system paths if still unresolved.  Only check
         * for package dependencies for RUNPATH paths.
         */
        'nextlib: for lib in elf.libraries {
            let mut found = false;

            /*
             * RUNPATH entries.
             */
            for rpath in &runpath {
                let mut libpath = PathBuf::from(rpath);
                libpath.push(lib);
                if libpath.exists() {
                    self.check_shlib(path, &libpath);
                    self.check_pkg(path, &libpath, cache);
                    found = true;
                    continue 'nextlib;
                }
            }

            /*
             * PLATFORM_RPATH entries.
             */
            for rpath in &syspath {
                let mut libpath = PathBuf::from(rpath);
                libpath.push(lib);
                if libpath.exists() {
                    self.check_shlib(path, &libpath);
                    found = true;
                    continue 'nextlib;
                }
            }

            if !found {
                println!("{}: missing library: {}", path.display(), lib);
            }
        }
    }
}
