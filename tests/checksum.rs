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
 * Invalid arguments.
 *
 * XXX: This is not fully compatible, checksum.awk exits 3, we exit 2, but
 * unfortunatly this is hardcoded in clap.
 */
#[test]
fn test_checksum_invalid_args() {
    let cmd = Command::new(MKTOOL)
        .arg("checksum")
        .output()
        .expect(format!("unable to exec {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(2));
}

/*
 * Test running with no input files.
 */
#[test]
fn test_checksum_no_input() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let cmd = Command::new(MKTOOL)
        .arg("checksum")
        .arg(distinfo)
        .output()
        .expect(format!("unable to exec {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(0));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());
}

/*
 * Pass a nonexistent distinfo file.
 */
#[test]
fn test_checksum_bad_distinfo() {
    let distinfo = PathBuf::from("/nonexistent");
    let output =
        format!("checksum: distinfo file missing: {}\n", distinfo.display());
    let cmd = Command::new(MKTOOL)
        .arg("checksum")
        .arg(distinfo)
        .output()
        .expect(format!("unable to exec {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(3));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert_eq!(cmd.stderr, output.as_bytes());
}

/*
 * Passing a file that doesn't exist in distinfo should print a message
 * to stderr and exit 2.
 */
#[test]
fn test_checksum_bad_distfile() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let cmd = Command::new(MKTOOL)
        .arg("checksum")
        .arg(distinfo.clone())
        .arg("foo")
        .output()
        .expect(format!("unable to exec {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(2));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert_eq!(
        cmd.stderr,
        "checksum: No checksum recorded for foo\n".as_bytes()
    );
}

/*
 * Tests against a valid full distfile test.
 */
#[test]
fn test_checksum_valid_distfile() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let output = format!(
        "{}\n{}\n",
        "=> Checksum BLAKE2s OK for digest1.txt",
        "=> Checksum SHA512 OK for digest1.txt"
    );
    let cmd = Command::new(MKTOOL)
        .arg("checksum")
        .arg(distinfo.clone())
        .arg("digest1.txt")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to exec {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(0));
    assert_eq!(cmd.stdout, output.as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());

    /*
     * Valid, but we only request a single algorithm each time.
     */
    let output = "=> Checksum BLAKE2s OK for digest1.txt\n";
    let cmd = Command::new(MKTOOL)
        .arg("checksum")
        .arg("-a")
        .arg("BLAKE2s")
        .arg(distinfo.clone())
        .arg("digest1.txt")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to exec {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(0));
    assert_eq!(cmd.stdout, output.as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());

    let output = "=> Checksum SHA512 OK for digest2.txt\n";
    let cmd = Command::new(MKTOOL)
        .arg("checksum")
        .arg("-a")
        .arg("SHA512")
        .arg(distinfo.clone())
        .arg("digest2.txt")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to exec {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(0));
    assert_eq!(cmd.stdout, output.as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());

    /*
     * Output should be in distinfo order, with errors last, regardless of
     * order of command line arguments.
     */
    let output = format!(
        "{}\n{}\n{}\n{}\n",
        "=> Checksum BLAKE2s OK for digest1.txt",
        "=> Checksum SHA512 OK for digest1.txt",
        "=> Checksum BLAKE2s OK for digest2.txt",
        "=> Checksum SHA512 OK for digest2.txt"
    );
    let outerr = "checksum: No checksum recorded for digest11.txt\n";
    let cmd = Command::new(MKTOOL)
        .arg("checksum")
        .arg(distinfo.clone())
        .arg("digest2.txt")
        .arg("digest11.txt")
        .arg("digest1.txt")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to exec {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(2));
    assert_eq!(cmd.stdout, output.as_bytes());
    assert_eq!(cmd.stderr, outerr.as_bytes());
}

/*
 * Test input from file / stdin.
 */
#[test]
fn test_checksum_input_file() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let tmpfile = tmpdir.join("test_checksum_input_file.txt");
    fs::write(&tmpfile, "digest2.txt\n").expect("unable to write temp file");
    let output = format!(
        "{}\n{}\n{}\n{}\n",
        "=> Checksum BLAKE2s OK for digest1.txt",
        "=> Checksum SHA512 OK for digest1.txt",
        "=> Checksum BLAKE2s OK for digest2.txt",
        "=> Checksum SHA512 OK for digest2.txt"
    );
    let cmd = Command::new(MKTOOL)
        .arg("checksum")
        .arg(distinfo.clone())
        .arg("-I")
        .arg(&tmpfile)
        .arg("digest1.txt")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    fs::remove_file(&tmpfile).expect("unable to remove temp file");
    assert_eq!(cmd.status.code(), Some(0));
    assert_eq!(cmd.stdout, output.as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());
}

#[test]
fn test_checksum_input_stdin() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let output = format!(
        "{}\n{}\n{}\n{}\n",
        "=> Checksum BLAKE2s OK for digest1.txt",
        "=> Checksum SHA512 OK for digest1.txt",
        "=> Checksum BLAKE2s OK for digest2.txt",
        "=> Checksum SHA512 OK for digest2.txt"
    );
    let mut cmd = Command::new(MKTOOL)
        .arg("checksum")
        .arg(distinfo.clone())
        .arg("-I")
        .arg("-")
        .arg("digest2.txt")
        .current_dir("tests/data")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    let mut stdin = cmd.stdin.take().expect("failed to open stdin");
    std::thread::spawn(move || {
        stdin
            .write_all("digest1.txt".as_bytes())
            .expect("failed to write to stdin");
    });
    let out = cmd.wait_with_output().expect("failed to wait on child");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, output.as_bytes());
    assert_eq!(out.stderr, "".as_bytes());
}

/*
 * Test strip suffix mode.  Use filename digest1.txt.suffix but validate
 * against the digest1.txt distinfo entry.
 */
#[test]
fn test_checksum_strip_mode() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");

    let output = "=> Checksum SHA512 OK for digest1.txt\n";
    let cmd = Command::new(MKTOOL)
        .arg("checksum")
        .arg("-a")
        .arg("SHA512")
        .arg("-s")
        .arg(".suffix")
        .arg(distinfo.clone())
        .arg("digest1.txt.suffix")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to exec {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(0));
    assert_eq!(cmd.stdout, output.as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());

    /* Invalid suffix strip.  */
    let output = "checksum: No checksum recorded for digest1.txt.suffix\n";
    let cmd = Command::new(MKTOOL)
        .arg("checksum")
        .arg("-s")
        .arg(".badsuffix")
        .arg(distinfo.clone())
        .arg("digest1.txt.suffix")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to exec {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(2));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert_eq!(cmd.stderr, output.as_bytes());
}

/*
 * No checksum type recorded.
 */
#[test]
fn test_checksum_no_checksum() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let output = format!(
        "{}\n{}\n",
        "checksum: No SHA1 checksum recorded for digest1.txt",
        "checksum: No SHA1 checksum recorded for nonexistent.txt",
    );
    let cmd = Command::new(MKTOOL)
        .arg("checksum")
        .arg("-a")
        .arg("SHA1")
        .arg(distinfo.clone())
        .arg("digest1.txt")
        .arg("nonexistent.txt")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to exec {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(2));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert_eq!(cmd.stderr, output.as_bytes());
}

/*
 * Patch mode.
 */
#[test]
fn test_checksum_patch_mode() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let output = "=> Checksum SHA1 OK for patch-Makefile\n";
    let cmd = Command::new(MKTOOL)
        .arg("checksum")
        .arg("-p")
        .arg(distinfo.clone())
        .arg("patch-Makefile")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to exec {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(0));
    assert_eq!(cmd.stdout, output.as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());
}

/*
 * Now run some of the same tests but with a modified distinfo file containing
 * invalid checksums.
 */
#[test]
fn test_checksum_mismatch() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo.bad");

    // Only the first checksum mismatch is printed on failure.
    let output = "checksum: Checksum BLAKE2s mismatch for digest1.txt\n";
    let cmd = Command::new(MKTOOL)
        .arg("checksum")
        .arg(distinfo.clone())
        .arg("digest1.txt")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to exec {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(1));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert_eq!(cmd.stderr, output.as_bytes());

    let output = "checksum: Checksum SHA512 mismatch for digest1.txt\n";
    let cmd = Command::new(MKTOOL)
        .arg("checksum")
        .arg("-a")
        .arg("SHA512")
        .arg(distinfo.clone())
        .arg("digest1.txt")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to exec {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(1));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert_eq!(cmd.stderr, output.as_bytes());

    let output = "checksum: Checksum SHA1 mismatch for patch-Makefile\n";
    let cmd = Command::new(MKTOOL)
        .arg("checksum")
        .arg("-p")
        .arg(distinfo.clone())
        .arg("patch-Makefile")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to exec {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(1));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert_eq!(cmd.stderr, output.as_bytes());
}
