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

use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const MKTOOL: &str = env!("CARGO_BIN_EXE_mktool");

/*
 * Simple dst1 -> src1
 */
#[test]
fn test_symlink_simple() {
    let tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let mut cmd = Command::new(MKTOOL)
        .arg("symlinks")
        .current_dir(tmpdir.clone())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    let mut stdin = cmd.stdin.take().expect("failed to open stdin");
    std::thread::spawn(move || {
        stdin
            .write_all("dst1 -> src1\n".as_bytes())
            .expect("failed to write to stdin");
    });
    let out = cmd.wait_with_output().expect("failed to wait on child");

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, "".as_bytes());
    assert_eq!(out.stderr, "".as_bytes());
    assert_eq!(tmpdir.clone().join("dst1").is_symlink(), true);
    assert_eq!(tmpdir.clone().join("dst1").exists(), false);
    assert_eq!(tmpdir.clone().join("src1").exists(), false);
}

/*
 * Recreating an existing symlink should work, ln -fs style.
 */
#[test]
fn test_symlink_overwrite() {
    let tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let mut cmd = Command::new(MKTOOL)
        .arg("symlinks")
        .current_dir(tmpdir.clone())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    let mut stdin = cmd.stdin.take().expect("failed to open stdin");
    std::thread::spawn(move || {
        stdin
            .write_all("dst2 -> src2\ndst2 -> src2a\n".as_bytes())
            .expect("failed to write to stdin");
    });
    let out = cmd.wait_with_output().expect("failed to wait on child");

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, "".as_bytes());
    assert_eq!(out.stderr, "".as_bytes());
    assert_eq!(tmpdir.clone().join("dst2").is_symlink(), true);
    assert_eq!(tmpdir.clone().join("dst2").exists(), false);
    assert_eq!(tmpdir.clone().join("src2").exists(), false);
    assert_eq!(tmpdir.clone().join("src2a").exists(), false);
}

/*
 * Require creating a directory tree first.
 */
#[test]
fn test_symlink_subdir() {
    let tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let mut cmd = Command::new(MKTOOL)
        .arg("symlinks")
        .current_dir(tmpdir.clone())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    let mut stdin = cmd.stdin.take().expect("failed to open stdin");
    std::thread::spawn(move || {
        // While here test that whitespace is trimmed.
        stdin
            .write_all(" dst3/a/b/c/f  ->  src3 \n".as_bytes())
            .expect("failed to write to stdin");
    });
    let out = cmd.wait_with_output().expect("failed to wait on child");

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, "".as_bytes());
    assert_eq!(out.stderr, "".as_bytes());
    assert_eq!(tmpdir.clone().join("dst3").is_dir(), true);
    assert_eq!(tmpdir.clone().join("dst3/a").is_dir(), true);
    assert_eq!(tmpdir.clone().join("dst3/a/b").is_dir(), true);
    assert_eq!(tmpdir.clone().join("dst3/a/b/c").is_dir(), true);
    assert_eq!(tmpdir.clone().join("dst3/a/b/c/f").is_symlink(), true);
    assert_eq!(tmpdir.clone().join("dst3/a/b/c/f").exists(), false);
}

/*
 * Invalid lines are simply ignored.
 */
#[test]
fn test_symlink_invalid() {
    let mut tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    /*
     * Create a temporary directory to run the tests in.  As no files or
     * symlinks should be created, we should be able to remove the directory
     * at the end with no errors.
     */
    tmpdir.push("invalid");
    fs::create_dir(&tmpdir).expect("unable to create directory");
    let mut cmd = Command::new(MKTOOL)
        .arg("symlinks")
        .current_dir(tmpdir.clone())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    let mut stdin = cmd.stdin.take().expect("failed to open stdin");
    std::thread::spawn(move || {
        stdin
            .write_all(
                format!(
                    "{}\n{}\n{}\n\n",
                    "one/two -> three -> four", "one", "one two"
                )
                .as_bytes(),
            )
            .expect("failed to write to stdin");
    });
    let out = cmd.wait_with_output().expect("failed to wait on child");

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, "".as_bytes());
    assert_eq!(out.stderr, "".as_bytes());

    fs::remove_dir(&tmpdir).expect("unable to remove directory");
}
