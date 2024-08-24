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
use std::fs;
use std::io::{self, BufRead};
use std::os::unix;
use std::path::PathBuf;

#[derive(Args, Debug)]
pub struct Symlinks {}

impl Symlinks {
    pub fn run(&self) -> Result<i32, Box<dyn std::error::Error>> {
        for line in io::stdin().lock().lines() {
            let line = line?;
            let mut p = line.split(" -> ");
            if p.clone().count() != 2 {
                continue;
            }
            if let (Some(l), Some(o)) = (p.next(), p.next()) {
                let link = PathBuf::from(l.trim());
                let original = PathBuf::from(o.trim());
                /*
                 * Create any parent directories required as part of the
                 * target.
                 */
                if let Some(dir) = link.parent() {
                    if dir.as_os_str() != "" {
                        fs::create_dir_all(dir)?;
                    }
                }
                /*
                 * Ignore errors, just try to remove the destination (we are
                 * essentially operating like "ln -fs").  Ideally we'd just
                 * ignore ENOENT, but we'll soon find out about other problems
                 * when we try to create the link.
                 */
                let _ = fs::remove_file(&link);
                unix::fs::symlink(original, link)?;
            }
        }
        Ok(0)
    }
}
