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

use crate::check_shlibs::{check_pkg, check_shlib};
use crate::check_shlibs::{CheckShlibs, CheckState};
use goblin::elf::Elf;
use std::env;
use std::path::{Path, PathBuf};

impl CheckShlibs {
    pub fn check_dso(
        &self,
        path: &Path,
        object: &[u8],
        state: &mut CheckState,
    ) {
        let elf = match Elf::parse(object) {
            Ok(o) => o,
            Err(_) => return,
        };
        let runpath: Vec<String> = match elf.runpaths.first() {
            Some(p) => p.split(':').map(|s| s.to_string()).collect(),
            None => vec![],
        };

        /*
         * System paths are prefixed with CROSS_DESTDIR, if set.
         */
        let mut syspath: Vec<PathBuf> = vec![];
        if let Ok(paths) = env::var("PLATFORM_RPATH") {
            let cross_prefix = match &state.cross_destdir {
                Some(p) => p.clone(),
                None => PathBuf::new(),
            };
            for p in paths.split(':').collect::<Vec<&str>>() {
                let mut path = cross_prefix.clone();
                path.push(p);
                syspath.push(path);
            }
        }

        /*
         * With ELF we have a list of library requirements, and a list of paths
         * to search for them.  Try the paths from RUNPATH first, before
         * falling back to the system paths if still unresolved.  Only check
         * for package dependencies for RUNPATH paths.
         */
        'nextlib: for lib in elf.libraries {
            /*
             * RUNPATH entries.
             */
            for rpath in &runpath {
                let mut libpath = PathBuf::from(rpath);
                libpath.push(lib);
                let exists = match state.statlibs.get(&libpath) {
                    Some(e) => *e,
                    None => {
                        let e = libpath.exists();
                        state.statlibs.insert(libpath.to_path_buf(), e);
                        e
                    }
                };
                if exists {
                    check_shlib(path, &libpath, state);
                    check_pkg(path, &libpath, state);
                    continue 'nextlib;
                }
            }

            /*
             * Look inside DESTDIR for any RUNPATH entries that haven't been
             * installed yet.  All we can do is check for existence, as they
             * will clearly fall foul of e.g. WRKDIR checks.
             */
            for rpath in &runpath {
                let mut libpath = state.destdir.clone();
                let rp = PathBuf::from(rpath);
                match rp.strip_prefix("/") {
                    Ok(p) => libpath.push(p),
                    Err(_) => libpath.push(rp),
                }
                libpath.push(lib);
                let exists = match state.statlibs.get(&libpath) {
                    Some(e) => *e,
                    None => {
                        let e = libpath.exists();
                        state.statlibs.insert(libpath.to_path_buf(), e);
                        e
                    }
                };
                if exists {
                    continue 'nextlib;
                }
            }

            /*
             * PLATFORM_RPATH entries.  As per above these are prefixed with
             * CROSS_DESTDIR if that is set, and we only perform shlib checks
             * on them.
             */
            for rpath in &syspath {
                let mut libpath = PathBuf::from(rpath);
                libpath.push(lib);
                let exists = match state.statlibs.get(&libpath) {
                    Some(e) => *e,
                    None => {
                        let e = libpath.exists();
                        state.statlibs.insert(libpath.to_path_buf(), e);
                        e
                    }
                };
                if exists {
                    check_shlib(path, &libpath, state);
                    continue 'nextlib;
                }
            }

            /*
             * If we're still here it wasn't found.
             */
            println!("{}: missing library: {}", path.display(), lib);
        }
    }
}
