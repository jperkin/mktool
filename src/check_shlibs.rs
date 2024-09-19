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

#[cfg(all(unix, not(target_os = "macos")))]
mod elf;
#[cfg(target_os = "macos")]
mod macho;

use clap::Args;
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Args, Debug)]
pub struct CheckShlibs {
    #[arg(short = 'j', value_name = "jobs")]
    #[arg(help = "Maximum number of threads (or \"MKTOOL_JOBS\" env var)")]
    jobs: Option<usize>,
}

/*
 * Shared state for checks.
 */
pub struct CheckCache {
    pkginfo: PathBuf,
    /* _RRDEPENDS_FILE format: "deptype pkgmatch pkg" */
    depends: Vec<(String, String, String)>,
    /* Have we already tested for this library path existence? */
    statlibs: HashMap<PathBuf, bool>,
    /* Have we already resolved this library path to a package name? */
    pkglibs: HashMap<PathBuf, Option<String>>,
}

/**
 * See if this library path belongs to a package.  If it does, ensure
 * that the package is a runtime dependency.
 */
fn check_pkg<P1, P2>(obj: P1, lib: P2, cache: &mut CheckCache) -> bool
where
    P1: AsRef<Path>,
    P2: AsRef<Path>,
{
    /*
     * Look for an existing cached entry for this library.
     */
    let pkgname = if let Some(entry) = cache.pkglibs.get(lib.as_ref()) {
        match entry {
            Some(p) => p.to_string(),
            /* Not a pkgsrc library, return early. */
            None => return true,
        }
    } else {
        /*
         * No cached entry, execute pkg_info to find out if it's a
         * pkgsrc library and store back to the cache accordingly.
         */
        let cmd = Command::new(&cache.pkginfo)
            .arg("-Fe")
            .arg(lib.as_ref())
            .output()
            .expect("Unable to execute pkg_info");

        if let Some(0) = cmd.status.code() {
            let p = String::from_utf8(cmd.stdout)
                .expect("Invalid pkgname")
                .trim()
                .to_string();
            cache
                .pkglibs
                .insert(lib.as_ref().to_path_buf(), Some(p.clone()));
            p
        } else {
            cache.pkglibs.insert(lib.as_ref().to_path_buf(), None);
            return true;
        }
    };

    /*
     * If we depend on a pkgsrc library then it must be a full
     * dependency.  Verify that it is.
     */
    for dep in &cache.depends {
        if dep.2 == pkgname && (dep.0 == "full" || dep.0 == "implicit-full") {
            return true;
        }
    }

    /*
     * If we didn't already exit early then this is a pkgsrc dependency
     * that is not correctly registered.
     */
    println!(
        "{}: {}: {} is not a runtime dependency",
        obj.as_ref().display(),
        lib.as_ref().display(),
        pkgname
    );
    false
}

fn check_shlib<P1, P2>(obj: P1, lib: P2) -> bool
where
    P1: AsRef<Path>,
    P2: AsRef<Path>,
{
    let mut rv = true;

    /*
     * Library paths must not start with WRKDIR.
     */
    if let Ok(wrkdir) = std::env::var("WRKDIR") {
        if lib.as_ref().starts_with(wrkdir) {
            println!(
                "{}: path relative to WRKDIR: {}",
                obj.as_ref().display(),
                lib.as_ref().display()
            );
            rv = false;
        }
    }

    /*
     * Library paths must be absolute.
     */
    if !lib.as_ref().starts_with("/") {
        println!(
            "{}: relative library path: {}",
            obj.as_ref().display(),
            lib.as_ref().display()
        );
        rv = false;
    }

    rv
}

impl CheckShlibs {
    pub fn run(&self) -> Result<i32, Box<dyn std::error::Error>> {
        /*
         * First verify that we have all the required environment variables
         * set, and perform initial configuration of them.
         */
        let pkginfo = match std::env::var("PKG_INFO_CMD") {
            Ok(s) => PathBuf::from(s),
            Err(e) => {
                eprintln!("Could not read PKG_INFO_CMD: {e}");
                std::process::exit(1);
            }
        };
        let depends = match std::env::var("DEPENDS_FILE") {
            Ok(s) => {
                let mut deps = vec![];
                let f = fs::read_to_string(s)?;
                for line in f.lines() {
                    let fields: Vec<_> = line.split_whitespace().collect();
                    if fields.len() != 3 {
                        eprintln!("Bad DEPENDS_FILE input?");
                        std::process::exit(1);
                    }
                    let deptype = fields[0].to_string();
                    let pkgmatch = fields[1].to_string();
                    let pkg = fields[2].to_string();
                    deps.push((deptype, pkgmatch, pkg))
                }
                deps
            }
            Err(e) => {
                eprintln!("Could not read DEPENDS_FILE: {e}");
                std::process::exit(1);
            }
        };

        let mut cache = CheckCache {
            pkginfo,
            depends,
            statlibs: HashMap::new(),
            pkglibs: HashMap::new(),
        };

        /*
         * Ok let's go.
         */
        for line in io::stdin().lock().lines() {
            let line = line?;
            let path = Path::new(&line);
            if let Ok(dso) = fs::read(path) {
                self.check_dso(path, &dso, &mut cache);
            }
        }

        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shlib() {
        let obj = "/opt/pkg/bin/mutt";
        /*
         * Library paths must be absolute.
         */
        assert_eq!(check_shlib(obj, "libfoo.so"), false);
        /*
         * Library paths must not start with WRKDIR
         */
        unsafe {
            std::env::set_var("WRKDIR", "/wrk");
        }
        assert_eq!(check_shlib(obj, "/wrk/libfoo.so"), false);
        assert_eq!(check_shlib(obj, "/libfoo.so"), true);
    }
}
