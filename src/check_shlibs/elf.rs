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
use goblin::elf::Elf;
use std::env;
use std::path::Path;

impl CheckShlibs {
    pub fn verify_dso(&self, path: &Path, object: &[u8]) {
        let elf = match Elf::parse(&object) {
            Ok(o) => ok,
            Err(_) => return,
        };
        for lib in elf.libraries {
            self.verify_lib(path, lib);
        }
    }

    fn verify_lib(&self, obj: &Path, lib: &str) {
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
