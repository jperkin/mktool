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

use crate::check_shlibs::CheckShlibs;
use goblin::mach::{Mach, SingleArch};
use std::env;
use std::path::Path;

impl CheckShlibs {
    pub fn verify_dso(&self, path: &Path, object: &[u8]) {
        let pobj = match Mach::parse(object) {
            Ok(o) => o,
            Err(_) => return,
        };
        let obj = match pobj {
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
            self.verify_lib(path, lib);
        }
    }

    fn verify_lib(&self, obj: &Path, lib: &str) {
        /*
         * Skip system libraries if requested on newer macOS.  Apple no
         * longer ship the actual file system entries (because lol) so the
         * existence test later on will fail.
         */
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
}
