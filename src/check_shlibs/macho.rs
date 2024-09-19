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
use goblin::mach::{Mach, SingleArch};
use std::path::Path;

impl CheckShlibs {
    pub fn check_dso(
        &self,
        path: &Path,
        object: &[u8],
        state: &mut CheckState,
    ) {
        let pobj = match Mach::parse(object) {
            Ok(o) => o,
            Err(_) => return,
        };
        let obj = match pobj {
            /*
             * XXX: Support Universal binaries correctly.  It's unlikely we'll
             * encounter these in pkgsrc at present as there's no multiarch
             * support.
             */
            Mach::Fat(fat) => {
                if let Ok(SingleArch::MachO(o)) = fat.get(0) {
                    o
                } else {
                    return;
                }
            }
            Mach::Binary(bin) => bin,
        };
        for (i, lib) in obj.libs.into_iter().enumerate() {
            /* Always skip the first entry on macOS, "self" */
            if i == 0 {
                continue;
            }

            /*
             * Skip system libraries if requested on newer macOS.  Apple no
             * longer ship the actual file system entries (because lol) so any
             * existence test later on will fail.
             */
            if std::env::var("SKIP_SYSTEM_LIBS").is_ok()
                && (lib.starts_with("/System/Library")
                    || lib.starts_with("/usr/lib"))
            {
                continue;
            }

            /*
             * As library paths on macOS are always fully specified, check that
             * they exist, caching seen entries - stat isn't cheap!
             */
            let libpath = Path::new(lib);
            let exists = match state.statlibs.get(libpath) {
                Some(e) => *e,
                None => {
                    let e = libpath.exists();
                    state.statlibs.insert(libpath.to_path_buf(), e);
                    e
                }
            };

            if !exists {
                println!("{}: missing library: {}", path.display(), lib);
                continue;
            }

            /*
             * File exists, perform full checks.
             */
            check_shlib(path, libpath, state);
            check_pkg(path, libpath, state);
        }
    }
}