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

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/*
 * Simple dst1 -> src1
 */
#[test]
fn test_symlink_simple() -> Result<()> {
    let tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let mut cmd = Command::new(MKTOOL)
        .arg("symlinks")
        .current_dir(&tmpdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut stdin = cmd.stdin.take().ok_or("failed to open stdin")?;
    std::thread::spawn(move || {
        let _ = stdin.write_all("dst1 -> src1\n".as_bytes());
    });
    let out = cmd.wait_with_output()?;

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, "".as_bytes());
    assert_eq!(out.stderr, "".as_bytes());
    assert!(tmpdir.clone().join("dst1").is_symlink());
    assert!(!tmpdir.clone().join("dst1").exists());
    assert!(!tmpdir.clone().join("src1").exists());
    Ok(())
}

/*
 * Recreating an existing symlink should work, ln -fs style.
 */
#[test]
fn test_symlink_overwrite() -> Result<()> {
    let tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let mut cmd = Command::new(MKTOOL)
        .arg("symlinks")
        .current_dir(&tmpdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut stdin = cmd.stdin.take().ok_or("failed to open stdin")?;
    std::thread::spawn(move || {
        let _ = stdin.write_all("dst2 -> src2\ndst2 -> src2a\n".as_bytes());
    });
    let out = cmd.wait_with_output()?;

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, "".as_bytes());
    assert_eq!(out.stderr, "".as_bytes());
    assert!(tmpdir.clone().join("dst2").is_symlink());
    assert!(!tmpdir.clone().join("dst2").exists());
    assert!(!tmpdir.clone().join("src2").exists());
    assert!(!tmpdir.clone().join("src2a").exists());
    Ok(())
}

/*
 * Require creating a directory tree first.
 */
#[test]
fn test_symlink_subdir() -> Result<()> {
    let tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let mut cmd = Command::new(MKTOOL)
        .arg("symlinks")
        .current_dir(&tmpdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut stdin = cmd.stdin.take().ok_or("failed to open stdin")?;
    std::thread::spawn(move || {
        // While here test that whitespace is trimmed.
        let _ = stdin.write_all(" dst3/a/b/c/f  ->  src3 \n".as_bytes());
    });
    let out = cmd.wait_with_output()?;

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, "".as_bytes());
    assert_eq!(out.stderr, "".as_bytes());
    assert!(tmpdir.clone().join("dst3").is_dir());
    assert!(tmpdir.clone().join("dst3/a").is_dir());
    assert!(tmpdir.clone().join("dst3/a/b").is_dir());
    assert!(tmpdir.clone().join("dst3/a/b/c").is_dir());
    assert!(tmpdir.clone().join("dst3/a/b/c/f").is_symlink());
    assert!(!tmpdir.clone().join("dst3/a/b/c/f").exists());
    Ok(())
}

/*
 * Invalid lines are simply ignored.
 */
#[test]
fn test_symlink_invalid() -> Result<()> {
    let mut tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    /*
     * Create a temporary directory to run the tests in.  As no files or
     * symlinks should be created, we should be able to remove the directory
     * at the end with no errors.
     */
    tmpdir.push("invalid");
    fs::create_dir(&tmpdir)?;
    let mut cmd = Command::new(MKTOOL)
        .arg("symlinks")
        .current_dir(&tmpdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut stdin = cmd.stdin.take().ok_or("failed to open stdin")?;
    std::thread::spawn(move || {
        let _ = stdin.write_all(
            format!(
                "{}\n{}\n{}\n\n",
                "one/two -> three -> four", "one", "one two"
            )
            .as_bytes(),
        );
    });
    let out = cmd.wait_with_output()?;

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, "".as_bytes());
    assert_eq!(out.stderr, "".as_bytes());

    fs::remove_dir(&tmpdir)?;
    Ok(())
}
