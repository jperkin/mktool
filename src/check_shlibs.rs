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
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Args, Debug)]
pub struct CheckShlibs {}

/*
 * Shared state for checks.
 */
pub struct CheckState {
    pkg_info_cmd: PathBuf,
    pkg_info_args: Vec<String>,
    /* _RRDEPENDS_FILE format: "deptype pkgmatch pkg" */
    depends: Vec<(String, String, String)>,
    /* List of toxic library path matches */
    toxic: Vec<Regex>,
    /* Have we already tested for this library path existence? */
    statlibs: HashMap<PathBuf, bool>,
    /* Have we already resolved this library path to a package name? */
    pkglibs: HashMap<PathBuf, Option<String>>,
}

/**
 * See if this library path belongs to a package.  If it does, ensure
 * that the package is a runtime dependency.
 */
fn check_pkg<P1, P2>(obj: P1, lib: P2, state: &mut CheckState) -> bool
where
    P1: AsRef<Path>,
    P2: AsRef<Path>,
{
    /*
     * Look for an existing cached entry for this library.
     */
    let pkgname = if let Some(entry) = state.pkglibs.get(lib.as_ref()) {
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
        let cmd = Command::new(&state.pkg_info_cmd)
            .args(&state.pkg_info_args)
            .arg("-Fe")
            .arg(lib.as_ref())
            .output()
            .expect("Unable to execute pkg_info");

        if let Some(0) = cmd.status.code() {
            let p = String::from_utf8(cmd.stdout)
                .expect("Invalid pkgname")
                .trim()
                .to_string();
            state
                .pkglibs
                .insert(lib.as_ref().to_path_buf(), Some(p.clone()));
            p
        } else {
            state.pkglibs.insert(lib.as_ref().to_path_buf(), None);
            return true;
        }
    };

    /*
     * If we depend on a pkgsrc library then it must be a full
     * dependency.  Verify that it is.
     */
    for dep in &state.depends {
        if dep.2 == pkgname && (dep.0 == "full" || dep.0 == "implicit-full") {
            return true;
        }
    }

    /*
     * If we didn't already exit early then this is a pkgsrc dependency that
     * is not correctly registered.
     */
    println!(
        "{}: {}: {} is not a runtime dependency",
        obj.as_ref().display(),
        lib.as_ref().display(),
        pkgname
    );
    false
}

fn check_shlib<P1, P2>(obj: P1, lib: P2, state: &CheckState) -> bool
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
     * Library paths must not match any that the user has marked as toxic, for
     * example if we want to explicitly avoid linking against certain system
     * libraries.
     */
    for regex in &state.toxic {
        if regex.is_match(&lib.as_ref().to_string_lossy()) {
            println!(
                "{}: resolved path {} matches toxic {}",
                obj.as_ref().display(),
                lib.as_ref().display(),
                regex
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
        let pkg_info_cmd: PathBuf;
        let mut pkg_info_args = vec![];
        match std::env::var("PKG_INFO_CMD") {
            Ok(s) => {
                let v: Vec<_> = s.split_whitespace().collect();
                if let Some((first, args)) = v.split_first() {
                    pkg_info_cmd = PathBuf::from(first);
                    for arg in args {
                        pkg_info_args.push(arg.to_string());
                    }
                } else {
                    eprintln!("Malformed PKG_INFO_CMD {s}");
                    std::process::exit(1);
                }
            }
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
        /*
         * These environment variables are optional.
         */
        let toxic = match std::env::var("CHECK_SHLIBS_TOXIC") {
            Ok(s) => {
                let mut v = vec![];
                let rgxs: Vec<_> = s.split_whitespace().collect();
                for r in rgxs {
                    let rgx = Regex::new(r).unwrap();
                    v.push(rgx);
                }
                v
            }
            Err(_) => {
                vec![]
            }
        };

        let mut state = CheckState {
            pkg_info_cmd,
            pkg_info_args,
            depends,
            toxic,
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
                self.check_dso(path, &dso, &mut state);
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
        let state = CheckState {
            pkg_info_cmd: PathBuf::from("/notyet"),
            pkg_info_args: vec![],
            depends: vec![],
            toxic: vec![
                Regex::new("libtoxic.so").unwrap(),
                Regex::new("^/toxic").unwrap(),
            ],
            statlibs: HashMap::new(),
            pkglibs: HashMap::new(),
        };

        let obj = "/opt/pkg/bin/mutt";
        /*
         * Library paths must be absolute.
         */
        assert_eq!(check_shlib(obj, "libfoo.so", &state), false);
        /*
         * Library paths must avoid toxic paths.
         */
        assert_eq!(check_shlib(obj, "/libtoxic.so", &state), false);
        assert_eq!(check_shlib(obj, "/toxic/lib.so", &state), false);
        /*
         * Library paths must not start with WRKDIR
         */
        unsafe {
            std::env::set_var("WRKDIR", "/wrk");
        }
        assert_eq!(check_shlib(obj, "/wrk/libfoo.so", &state), false);
        /*
         * These should be fine.
         */
        assert_eq!(check_shlib(obj, "/libfoo.so", &state), true);
        assert_eq!(check_shlib(obj, "/libnottoxic.so", &state), true);

        /*
         * Uncomment this to verify stdout.
         */
        //assert!(false);
    }
}
