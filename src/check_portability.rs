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

extern crate glob;

use clap::Args;
use content_inspector::{ContentType, inspect};
use std::fs;
use std::io::{BufRead, BufReader, Read};
use walkdir::WalkDir;

#[derive(Args, Debug)]
pub struct Cmd {}

fn check_random(line: &str) -> bool {
    let mut rv = false;
    let matches: Vec<_> = line.match_indices("$RANDOM").collect();
    if matches.is_empty() {
        return false;
    }
    for m in &matches {
        let start = m.0;
        let next = start + "$RANDOM".len();

        /*
         * $RANDOM mixed with PID ($$) is commonly found in GNU configure
         * scripts, and because they are always executed using a compatible
         * shell then are considered acceptable.  Turning this off produces
         * lots of false positives in e.g. config.guess.
         */
        if start >= 3 && line[start - 3..start] == *"$$-" {
            return false;
        }
        if next + 2 < line.len() && line[next..next + 3] == *"-$$" {
            return false;
        }

        /*
         * Trailing A-Z_, i.e. a variable that starts "$RANDOM.." such as
         * $RANDOMIZE is considered acceptable, but only if there is no bare
         * $RANDOM elsewhere on the line, so continue to other matches.
         */
        if next < line.len() {
            if let Some(ch) = line.chars().nth(next) {
                if ch.is_ascii_uppercase() || ch == '_' {
                    continue;
                }
            }
        }

        /*
         * If we're still here then there's another $RANDOM on the line and
         * we didn't already exit early for the acceptable cases.  Set exit
         * status that will be used unless we exit early later.
         */
        rv = true;
    }

    rv
}

fn check_test_eq(line: &str) -> bool {
    let words: Vec<_> = line.split_whitespace().collect();
    let mut idx = 2;
    while idx < words.len() {
        if words[idx] == "=="
            && (words[idx - 2] == "test" || words[idx - 2] == "[")
        {
            return true;
        }
        idx += 1;
    }
    false
}

fn print_random_warning() {
    let msg = r#"
Explanation:
===========================================================================
The variable $RANDOM is not required for a POSIX-conforming shell, and
many implementations of /bin/sh do not support it. It should therefore
not be used in shell programs that are meant to be portable across a
large number of POSIX-like systems.
===========================================================================
    "#;
    println!("{msg}");
}

fn print_test_eq_error() {
    let msg = r#"
Explanation:
===========================================================================
The "test" command, as well as the "[" command, are not required to know
the "==" operator. Only a few implementations like bash and some
versions of ksh support it.

When you run "test foo == foo" on a platform that does not support the
"==" operator, the result will be "false" instead of "true". This can
lead to unexpected behavior.

There are two ways to fix this error message. If the file that contains
the "test ==" is needed for building the package, you should create a
patch for it, replacing the "==" operator with "=". If the file is not
needed, add its name to the CHECK_PORTABILITY_SKIP variable in the
package Makefile.
===========================================================================
    "#;
    println!("{msg}");
}

impl Cmd {
    pub fn run(&self) -> Result<i32, Box<dyn std::error::Error>> {
        let mut rv = 0;

        /*
         * File globs to skip specified in CHECK_PORTABILITY_SKIP.
         */
        let mut skipglob = vec![];
        if let Ok(paths) = std::env::var("CHECK_PORTABILITY_SKIP") {
            for p in paths.split_whitespace().collect::<Vec<&str>>() {
                if let Ok(g) = glob::Pattern::new(p) {
                    skipglob.push(g);
                }
            }
        }

        /*
         * List of file extensions to skip.  These are plain strings rather
         * than adding to skipglob as it's faster.  Based on the lists in
         * check-portability.sh but with some additions.
         */
        let mut skipext: Vec<String> = vec![];
        skipext.push(".orig".to_string());
        skipext.push("~".to_string());
        for ext in [
            "1",
            "3",
            "C",
            "a",
            "ac",
            "c",
            "cc",
            "css",
            "cxx",
            "docbook",
            "dtd",
            "el",
            "f",
            "gif",
            "gn",
            "go",
            "gz",
            "h",
            "hpp",
            "htm",
            "html",
            "hxx",
            "idl",
            "inc",
            "jpg",
            "js",
            "json",
            "kicad_mod",
            "m4",
            "map",
            "md",
            "mo",
            "ogg",
            "page",
            "php",
            "pl",
            "png",
            "po",
            "properties",
            "py",
            "py",
            "rb",
            "result",
            "svg",
            "test",
            "tfm",
            "ts",
            "txt",
            "vf",
            "xml",
            "xpm",
        ] {
            skipext.push(format!(".{ext}"));
        }

        /*
         * Get list of patched files.
         */
        let mut patched = vec![];
        if let Ok(patchdir) = std::env::var("PATCHDIR") {
            for patch in
                WalkDir::new(patchdir).into_iter().filter_map(|e| e.ok())
            {
                if !patch.file_type().is_file() {
                    continue;
                }
                if !patch.file_name().to_string_lossy().starts_with("patch-") {
                    continue;
                }
                let pfile = fs::File::open(patch.path())?;
                let reader = BufReader::new(pfile);
                for line in reader.lines() {
                    let line = line?;
                    if line.starts_with("+++") {
                        let v: Vec<&str> =
                            line.splitn(2, char::is_whitespace).collect();
                        if v.len() == 2 {
                            patched.push(v[1].to_string());
                        }
                        break;
                    }
                }
            }
        }

        'nextfile: for entry in
            WalkDir::new(".").into_iter().filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }

            /*
             * Skip extensions we aren't interested in.
             */
            let fname: &str = &entry.file_name().to_string_lossy();
            for ext in &skipext {
                if fname.ends_with(ext) {
                    continue 'nextfile;
                }
            }

            /*
             * If this filename ends ".in" and we already have a patch for the
             * non-".in" filename then skip it, no need to patch both.
             */
            if let Some(p) =
                entry.file_name().to_string_lossy().strip_suffix(".in")
            {
                if patched.contains(&p.into()) {
                    continue 'nextfile;
                }
            }

            let path = entry.path();

            /*
             * Remove leading "./" from walkdir path entries as all
             * CHECK_PORTABILITY_SKIP matches are relative to WRKDIR.
             */
            let mpath = path.strip_prefix("./").unwrap();
            for g in &skipglob {
                if g.matches_path(mpath) {
                    continue 'nextfile;
                }
            }

            /*
             * Verify that the first 1KB of the file is valid UTF-8, and
             * contains a valid shell hashbang, otherwise skip to avoid
             * wasting time with binary files and non-shell files.
             *
             * XXX If CHECK_PORTABILITY_EXPERIMENTAL is enabled then we
             * should continue to check Makefiles (see shell version),
             * however that is not currently supported and may never be,
             * given I don't know anyone who enables it.
             */
            let mut file = fs::File::open(path)?;
            let mut buf = [0; 1024];
            let n = file.read(&mut buf)?;

            /*
             * Perform the simple and fast hashbang check first.
             */
            if !buf.starts_with(b"#!") {
                continue 'nextfile;
            }

            /*
             * More complicated check for "/bin/sh" somewhere on first line
             * next.
             */
            let binsh = b"/bin/sh";
            let mut lines = buf.splitn(2, |ch| *ch == b'\n');
            let first = lines.next().unwrap();
            if !first.windows(binsh.len()).any(|win| win == binsh) {
                continue 'nextfile;
            }

            if inspect(&buf[..n]) == ContentType::UTF_8 {
                /*
                 * XXX: can we be more efficient and avoid re-reading the
                 * first 1KB?
                 */
                let file = fs::File::open(path)?;
                let reader = BufReader::new(file);
                for (i, line) in reader.lines().enumerate() {
                    /*
                     * While the first 1KB may have been valid UTF-8 we
                     * cannot vouch for the remainder of the file, so skip
                     * any invalid lines.
                     */
                    if let Ok(line) = line {
                        /*
                         * Remove all leading and trailing whitespace to
                         * simplify matches, and ignore comments.
                         */
                        let line = line.trim();
                        if line.starts_with('#') {
                            continue;
                        }
                        if check_random(line) {
                            eprintln!(
                                "WARNING: [check-portability] => Found $RANDOM:"
                            );
                            eprintln!(
                                "WARNING: [check-portability] {}:{}: {}",
                                mpath.display(),
                                i + 1,
                                line
                            );
                            print_random_warning();
                        }
                        if check_test_eq(line) {
                            eprintln!(
                                "ERROR: [check-portability] => Found test ... == ...:"
                            );
                            eprintln!(
                                "ERROR: [check-portability] {}:{}: {}",
                                mpath.display(),
                                i + 1,
                                line
                            );
                            print_test_eq_error();
                            rv = 1;
                        }
                    }
                }
            }
        }

        Ok(rv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random() {
        assert!(check_random("$RANDOM"));

        /*
         * Only exact matches for prefix/suffix "$$" are valid.
         */
        assert!(check_random("-$RANDOM"));
        assert!(check_random("$-$RANDOM"));
        assert!(check_random("$RANDOM-"));
        assert!(check_random("$RANDOM-$"));
        assert!(!check_random("$$-$RANDOM"));
        assert!(!check_random("$RANDOM-$$"));

        /*
         * If we see GNU-style $$-$RANDOM anywhere then all other matches are
         * effectively ignored.
         */
        assert!(!check_random("$RANDOM-$$ $RANDOM"));
        assert!(!check_random("$RANDOM $RANDOM-$$"));

        /*
         * $RANDOM at the start of a variable name is fine, unless we also see
         * a bare $RANDOM too (this differs from check-portability.awk which
         * is first-match-wins).
         */
        assert!(!check_random("$RANDOMIZE"));
        assert!(!check_random("$RANDOM_ISH"));
        assert!(check_random("$RANDOMIZE $RANDOM"));

        /*
         * Commented matches are fine.  Unfortunately we strip commented
         * lines prior to calling check_random() currently, so this should
         * go into an integration test.
         */
        //assert_eq!(check_random("# $RANDOM"), false);
        //assert_eq!(check_random("   # $RANDOM"), false);

        /*
         * Misc non-matches.
         */
        assert!(!check_random(""));
        assert!(!check_random("RANDOM"));
        assert!(!check_random("$ RANDOM"));
    }

    #[test]
    fn test_eq() {
        assert!(check_test_eq("if [ foo == bar ]; then"));

        /* XXX: No support for whitespace in variable at present.  */
        assert!(!check_test_eq("if [ 'foo bar' == ojnk ]; then"));

        /*
         * Misc non-matches.
         */
        assert!(!check_test_eq(""));
        assert!(!check_test_eq("foo == bar"));
        assert!(!check_test_eq("if foo == bar"));
        assert!(!check_test_eq("if [ foo = bar ]; then"));
    }
}
