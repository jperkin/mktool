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

use anyhow::Context;
use clap::Args;
use regex::Regex;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Args, Debug)]
pub struct CheckShlibs {}

/*
 * Shared state for checks.
 */
pub struct CheckState {
    /*
     * CROSS_DESTDIR is used if packages are being cross-built and points to
     * the location of both system libraries and package dependencies.
     *
     * There's nothing technically stopping this from being supported on
     * macOS, it just isn't currently used so avoid a clippy warning.
     */
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    cross_destdir: Option<PathBuf>,
    /*
     * DESTDIR is the temporary directory that files are installed to prior to
     * being packaged and installed.
     */
    destdir: PathBuf,
    /*
     * List of system paths to look for libraries.  Not all file formats use
     * this, for example MachO uses absolute paths.
     */
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    system_paths: Vec<PathBuf>,
    /*
     * Where we built the package.  There should be no references to this path
     * at all in the final package.
     */
    wrkdir: PathBuf,
    /*
     * Additional work directories specified by the user which if matched will
     * result in failure.
     */
    wrkref: Vec<PathBuf>,
    /*
     * The contents of pkgdb.byfile.db for file -> pkg lookups.  pkg is an
     * Option so that we can cache negative lookups.
     */
    pkgdb: HashMap<PathBuf, Option<String>>,
    /*
     * Path to pkg_admin and any arguments (usually "-K /path/to/pkgdb")
     */
    pkg_admin_cmd: PathBuf,
    pkg_admin_args: Vec<String>,
    /*
     * The _RRDEPENDS_FILE.  Format is "deptype pkgmatch pkg"
     */
    depends: Vec<(String, String, String)>,
    /*
     * List of toxic library path regular expression matches.
     */
    toxic: Vec<Regex>,
    /*
     * Cache stat(2) lookups, storing whether file exists or not.
     */
    statlibs: HashMap<PathBuf, bool>,
}

/**
 * See if this library path belongs to a package.  If it does, ensure
 * that the package is a runtime dependency.
 */
fn check_pkg<P1, P2>(
    obj: P1,
    lib: P2,
    state: &mut CheckState,
) -> anyhow::Result<bool>
where
    P1: AsRef<Path>,
    P2: AsRef<Path>,
{
    /*
     * On first lookup we need to initialise the pkgdb.
     */
    if state.pkgdb.is_empty() {
        let cmd = Command::new(&state.pkg_admin_cmd)
            .args(&state.pkg_admin_args)
            .arg("dump")
            .output()
            .with_context(|| {
                format!("unable to execute {}", state.pkg_admin_cmd.display())
            })?;

        if let Some(0) = cmd.status.code() {
            /*
             * "pkg_admin dump" output format is generally of the form:
             *
             *   file: <filepath> pkg: <pkgname>
             *
             * However there are a couple of complications:
             *
             *   - File paths can contain spaces
             *   - There may be a "@pkgdir" string in front of the pkgname.
             *
             * So we extract everything in between "file: " and " pkg: " as
             * the filename, and then split by whitespace to get the last field
             * as the package name (as package names must not contain spaces).
             *
             * Bail early for anything that doesn't look right.
             */
            for line in cmd.stdout.split(|nl| *nl == b'\n') {
                if !line.starts_with(b"file: ") {
                    continue;
                }
                let Some(pos) = line.windows(6).position(|s| s == b" pkg: ")
                else {
                    continue;
                };
                let file =
                    PathBuf::from(OsString::from_vec((line[6..pos]).to_vec()));

                let Some(pkg) =
                    line.split(|sp| (*sp as char).is_whitespace()).next_back()
                else {
                    continue;
                };
                let pkg = match String::from_utf8(pkg.to_vec()) {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                state.pkgdb.insert(file, Some(pkg));
            }
        }
    }

    /*
     * Look for an existing cached entry for this library.  If there is no
     * existing entry, or an existing entry that is None, then either way it
     * is not a pkgsrc path and we should return early.
     */
    let pkgname = if let Some(entry) = state.pkgdb.get(lib.as_ref()) {
        match entry {
            Some(p) => p.to_string(),
            None => return Ok(true),
        }
    } else {
        state.pkgdb.insert(lib.as_ref().to_path_buf(), None);
        return Ok(true);
    };

    /*
     * If we depend on a pkgsrc library that appears in the depends file then
     * it must be a full dependency.  If the package is not listed in depends
     * then we don't currently have any good option other than to ignore it.
     * For example, if the current package being built is already installed,
     * then its installed libraries will be found, and of course a package
     * will never be listed in its own depends.
     */
    let mut found = false;
    for dep in &state.depends {
        if dep.2 == pkgname {
            found = true;
            if dep.0 == "full" || dep.0 == "indirect-full" {
                return Ok(true);
            }
        }
    }

    /*
     * Only issue an error if the package was listed in the depends file.
     */
    if found {
        println!(
            "{}: {}: {} is not a runtime dependency",
            obj.as_ref().display(),
            lib.as_ref().display(),
            pkgname
        );
    }
    Ok(false)
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
    if lib.as_ref().starts_with(&state.wrkdir) {
        println!(
            "{}: path relative to WRKDIR: {}",
            obj.as_ref().display(),
            lib.as_ref().display()
        );
        rv = false;
    }

    /*
     * Verify library does not match CHECK_WRKREF_EXTRA_DIRS.
     */
    for dir in &state.wrkref {
        if lib.as_ref().starts_with(dir) {
            println!(
                "{}: rpath {} relative to CHECK_WRKREF_EXTRA_DIRS directory {}",
                obj.as_ref().display(),
                lib.as_ref().display(),
                dir.display()
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
    pub fn run(&self) -> anyhow::Result<i32> {
        /*
         * First verify that we have all the required environment variables
         * set, and perform initial configuration of them.
         */
        let destdir = match std::env::var("DESTDIR") {
            Ok(s) => PathBuf::from(s),
            Err(_) => {
                eprintln!("DESTDIR is mandatory");
                std::process::exit(1);
            }
        };
        let cross_destdir = match std::env::var("CROSS_DESTDIR") {
            Ok(s) => {
                if s.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(s))
                }
            }
            Err(_) => {
                eprintln!("CROSS_DESTDIR is mandatory (can be empty)");
                std::process::exit(1);
            }
        };
        let wrkdir = match std::env::var("WRKDIR") {
            Ok(s) => PathBuf::from(s),
            Err(_) => {
                eprintln!("WRKDIR is mandatory");
                std::process::exit(1);
            }
        };
        let pkg_admin_cmd: PathBuf;
        let mut pkg_admin_args = vec![];
        match std::env::var("PKG_ADMIN_CMD") {
            Ok(s) => {
                let v: Vec<_> = s.split_whitespace().collect();
                if let Some((first, args)) = v.split_first() {
                    pkg_admin_cmd = PathBuf::from(first);
                    for arg in args {
                        pkg_admin_args.push(arg.to_string());
                    }
                } else {
                    eprintln!("Malformed PKG_ADMIN_CMD {s}");
                    std::process::exit(1);
                }
            }
            Err(_) => {
                eprintln!("PKG_ADMIN_CMD is mandatory");
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
            Err(_) => {
                eprintln!("DEPENDS_FILE is mandatory");
                std::process::exit(1);
            }
        };

        /*
         * These environment variables are optional.
         */
        let mut system_paths: Vec<PathBuf> = vec![];
        if let Ok(paths) = std::env::var("PLATFORM_RPATH") {
            let cross_prefix = match &cross_destdir {
                Some(p) => p.clone(),
                None => PathBuf::new(),
            };
            for p in paths.split(':').collect::<Vec<&str>>() {
                let mut path = cross_prefix.clone();
                path.push(p);
                system_paths.push(path);
            }
        }
        let toxic = match std::env::var("CHECK_SHLIBS_TOXIC") {
            Ok(s) => {
                let mut v = vec![];
                let rgxs: Vec<_> = s.split_whitespace().collect();
                for r in rgxs {
                    let rgx = Regex::new(r).with_context(|| {
                        format!("invalid CHECK_SHLIBS_TOXIC regex: {r}")
                    })?;
                    v.push(rgx);
                }
                v
            }
            Err(_) => {
                vec![]
            }
        };
        let wrkref = match std::env::var("CHECK_WRKREF_EXTRA_DIRS") {
            Ok(s) => {
                let mut v = vec![];
                let dirs: Vec<_> = s.split_whitespace().collect();
                for d in dirs {
                    v.push(PathBuf::from(d));
                }
                v
            }
            Err(_) => {
                vec![]
            }
        };

        let mut state = CheckState {
            destdir,
            cross_destdir,
            system_paths,
            wrkdir,
            wrkref,
            pkg_admin_cmd,
            pkg_admin_args,
            depends,
            toxic,
            statlibs: HashMap::new(),
            pkgdb: HashMap::new(),
        };

        /*
         * Ok let's go.
         */
        for line in io::stdin().lock().lines() {
            let line = line?;
            let path = Path::new(&line);
            match fs::read(path) {
                Ok(dso) => self.check_dso(path, &dso, &mut state)?,
                Err(e) => eprintln!("{}: {e}", path.display()),
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
            cross_destdir: None,
            destdir: PathBuf::from("/destdir"),
            system_paths: vec![],
            wrkdir: PathBuf::from("/wrkdir"),
            wrkref: vec![PathBuf::from("/wrkref")],
            pkg_admin_cmd: PathBuf::from("/notyet"),
            pkg_admin_args: vec![],
            depends: vec![],
            toxic: vec![
                Regex::new("libtoxic.so").unwrap(),
                Regex::new("^/toxic").unwrap(),
            ],
            statlibs: HashMap::new(),
            pkgdb: HashMap::new(),
        };

        let obj = "/opt/pkg/bin/mutt";
        /*
         * Library paths must be absolute.
         */
        assert!(!check_shlib(obj, "libfoo.so", &state));
        /*
         * Library paths must avoid toxic paths.
         */
        assert!(!check_shlib(obj, "/libtoxic.so", &state));
        assert!(!check_shlib(obj, "/toxic/lib.so", &state));
        /*
         * Library paths must not start with WRKDIR.
         */
        assert!(!check_shlib(obj, "/wrkdir/libfoo.so", &state));
        /*
         * Library paths must not match CHECK_WRKREF_EXTRA_DIRS.
         */
        assert!(!check_shlib(obj, "/wrkref/libfoo.so", &state));
        /*
         * These should be fine.
         */
        assert!(check_shlib(obj, "/libfoo.so", &state));
        assert!(check_shlib(obj, "/libnottoxic.so", &state));

        /*
         * Uncomment this to verify stdout.
         */
        //assert!(false);
    }
}
