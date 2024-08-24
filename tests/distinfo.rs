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
 * With no files it should just print an empty $NetBSD$ header and a
 * blank line, but exit 1.
 */
#[test]
fn test_distinfo_no_args() {
    let output = String::from("$NetBSD$\n\n");
    let cmd = Command::new(MKTOOL)
        .arg("distinfo")
        .output()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(1));
    assert_eq!(cmd.stdout, output.as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());
}

/*
 * With a valid distinfo but no files it should print nothing.
 */
#[test]
fn test_distinfo_just_distinfo() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");

    let cmd = Command::new(MKTOOL)
        .arg("distinfo")
        .arg("-f")
        .arg("distinfo")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(1));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());
}

/*
 * Invalid distdir / distinfo.
 */
#[test]
fn test_distinfo_invalid_distdir() {
    let cmd = Command::new(MKTOOL)
        .arg("distinfo")
        .arg("-d")
        .arg("/nonexistent")
        .output()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(128));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert!(cmd.stderr.starts_with(b"ERROR: Supplied DISTDIR"));
}

#[test]
fn test_distinfo_invalid_distinfo() {
    let cmd = Command::new(MKTOOL)
        .arg("distinfo")
        .arg("-f")
        .arg("/nonexistent")
        .output()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(128));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert!(cmd.stderr.starts_with(b"ERROR: Could not open distinfo"));
}

/*
 * Specify a distfile but no checksums, should only print size (and retain
 * existing patch entries).
 */
#[test]
fn test_distinfo_single_file_no_checksum() {
    let output = format!(
        "{}\n\n{}\n{}\n",
        "$NetBSD: distinfo,v 1.1 1970/01/01 00:00:00 ken Exp $",
        "Size (digest1.txt) = 159 bytes",
        "SHA1 (patch-Makefile) = ab5ce8a374d3aca7948eecabc35386d8195e3fbf",
    );
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");

    let cmd = Command::new(MKTOOL)
        .arg("distinfo")
        .arg("-f")
        .arg("distinfo")
        .arg("-c")
        .arg("digest1.txt")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(1));
    assert_eq!(cmd.stdout, output.as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());

    /*
     * Invalid distfiles are simply ignored.  Not sure this is ideal behaviour,
     * but this matches distinfo.awk.  Re-run the previous test with an extra
     * invalid argument, the output should be identical.
     */
    let cmd = Command::new(MKTOOL)
        .arg("distinfo")
        .arg("-f")
        .arg("distinfo")
        .arg("-c")
        .arg("digest1.txt")
        .arg("-c")
        .arg("does-not-exist.txt")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(1));
    assert_eq!(cmd.stdout, output.as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());
}

/*
 * Test a single patch, with and without an existing distinfo.  Without an
 * existing distinfo only the patch and an empty $NetBSD$ should be printed
 * with exit 1.  With an existing distinfo, as the patch contents are
 * identical, the entire distinfo should be printed with exit 0.
 */
#[test]
fn test_distinfo_single_patch_no_distinfo() {
    let output = format!(
        "{}\n\n{}\n",
        "$NetBSD$",
        "SHA1 (patch-Makefile) = ab5ce8a374d3aca7948eecabc35386d8195e3fbf",
    );

    let cmd = Command::new(MKTOOL)
        .arg("distinfo")
        .arg("-p")
        .arg("SHA1")
        .arg("patch-Makefile")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(1));
    assert_eq!(cmd.stdout, output.as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());
}

#[test]
fn test_distinfo_single_patch_with_distinfo() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let diout = fs::read(distinfo).expect("unable to read distinfo");
    let cmd = Command::new(MKTOOL)
        .arg("distinfo")
        .arg("-f")
        .arg("distinfo")
        .arg("-p")
        .arg("SHA1")
        .arg("patch-Makefile")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(0));
    assert_eq!(cmd.stdout, diout);
    assert_eq!(cmd.stderr, "".as_bytes());
}

/*
 * Test all known distfiles.  With distinfo the output should be identical and
 * exit 0, without should just print distfiles and exit 1.
 */
#[test]
fn test_distinfo_distfiles_no_distinfo() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let output = format!("{}\n\n{}\n{}\n{}\n{}\n{}\n{}\n",
        "$NetBSD$",
        "BLAKE2s (digest1.txt) = 54020b13a41ebeebdbec3910e60c13b024568e597aed3c3412e611f703590311",
        "SHA512 (digest1.txt) = ac6cd4956428e83cf6c13742d4d96af2608681d09def86fc8aaca0689af4d2fb09317692daf26c0301c79652c6f8fc3b2764a0b96e8b1bc897413ba46e520139",
        "Size (digest1.txt) = 159 bytes",
        "BLAKE2s (digest2.txt) = fb6527720b06b21010ddbac12cf2d3fed6b853f9ffef9915fb0757f4bef64335",
        "SHA512 (digest2.txt) = 1934886c6e69d65365124c67ff6c3b11a1eeee2ee2940376637344c0cb448cad910db1e7e59ce8b29e20f05c696a7a25cd9fe6367a8f4b10da7b86658ada251b",
        "Size (digest2.txt) = 165 bytes"
    );

    let cmd = Command::new(MKTOOL)
        .arg("distinfo")
        .arg("-a")
        .arg("BLAKE2s")
        .arg("-a")
        .arg("SHA512")
        .arg("-c")
        .arg("digest1.txt")
        .arg("-c")
        .arg("digest2.txt")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(1));
    assert_eq!(cmd.stdout, output.as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());
}

#[test]
fn test_distinfo_distfiles_with_distinfo() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let diout = fs::read(distinfo).expect("unable to read distinfo");

    let cmd = Command::new(MKTOOL)
        .arg("distinfo")
        .arg("-a")
        .arg("BLAKE2s")
        .arg("-a")
        .arg("SHA512")
        .arg("-c")
        .arg("digest1.txt")
        .arg("-c")
        .arg("digest2.txt")
        .arg("-f")
        .arg("distinfo")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(0));
    assert_eq!(cmd.stdout, diout);
    assert_eq!(cmd.stderr, "".as_bytes());
}

/*
 * Digest algorithms should be output in command line order.
 */
#[test]
fn test_distinfo_algorithm_order() {
    let cmd = Command::new(MKTOOL)
        .arg("distinfo")
        .arg("-a")
        .arg("SHA512")
        .arg("-a")
        .arg("BLAKE2s")
        .arg("-c")
        .arg("digest1.txt")
        .arg("-p")
        .arg("RMD160")
        .arg("-p")
        .arg("SHA1")
        .arg("patch-Makefile")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    let mut outlines = cmd.stdout.split(|c| *c == b'\n');
    assert_eq!(cmd.status.code(), Some(1));
    // Remember that .nth() consumes previous entries...
    assert!(outlines.nth(0).unwrap().starts_with(b"$NetBSD"));
    assert!(outlines.nth(1).unwrap().starts_with(b"SHA512"));
    assert!(outlines.nth(0).unwrap().starts_with(b"BLAKE2s"));
    assert!(outlines.nth(1).unwrap().starts_with(b"RMD160"));
    assert!(outlines.nth(0).unwrap().starts_with(b"SHA1"));
    assert_eq!(cmd.stderr, "".as_bytes());
}

/*
 * Test input from file / stdin.
 */
#[test]
fn test_distinfo_input_file() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let diout = fs::read(distinfo).expect("unable to read distinfo");
    let tmpdir: PathBuf = env::temp_dir();
    let tmpfile = tmpdir.join("test_distinfo_input_file.txt");
    fs::write(&tmpfile, "digest2.txt\ndigest1.txt\n")
        .expect("unable to write temp file");

    let cmd = Command::new(MKTOOL)
        .arg("distinfo")
        .arg("-a")
        .arg("BLAKE2s")
        .arg("-a")
        .arg("SHA512")
        .arg("-I")
        .arg(&tmpfile)
        .arg("-f")
        .arg("distinfo")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    fs::remove_file(&tmpfile).expect("unable to remove temp file");
    assert_eq!(cmd.status.code(), Some(0));
    assert_eq!(cmd.stdout, diout);
    assert_eq!(cmd.stderr, "".as_bytes());
}

#[test]
fn test_distinfo_input_stdin() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let diout = fs::read(distinfo).expect("unable to read distinfo");

    let mut cmd = Command::new(MKTOOL)
        .arg("distinfo")
        .arg("-a")
        .arg("BLAKE2s")
        .arg("-a")
        .arg("SHA512")
        .arg("-I")
        .arg("-")
        .arg("-f")
        .arg("distinfo")
        .current_dir("tests/data")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    let mut stdin = cmd.stdin.take().expect("failed to open stdin");
    std::thread::spawn(move || {
        stdin
            .write_all("digest1.txt\ndigest2.txt\n".as_bytes())
            .expect("failed to write to stdin");
    });
    let out = cmd.wait_with_output().expect("failed to wait on child");

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, diout);
    assert_eq!(out.stderr, "".as_bytes());
}

/*
 * Full test of all files.  The output should match the existing distinfo file
 * and so the exit status should be 0.
 */
#[test]
fn test_distinfo_full() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let diout = fs::read(distinfo).expect("unable to read distinfo");

    let cmd = Command::new(MKTOOL)
        .arg("distinfo")
        .arg("-a")
        .arg("BLAKE2s")
        .arg("-a")
        .arg("SHA512")
        .arg("-c")
        .arg("digest1.txt")
        .arg("-c")
        .arg("digest2.txt")
        .arg("-f")
        .arg("distinfo")
        .arg("-p")
        .arg("SHA1")
        .arg("patch-Makefile")
        .current_dir("tests/data")
        .output()
        .expect(format!("unable to spawn {}", MKTOOL).as_str());
    assert_eq!(cmd.status.code(), Some(0));
    assert_eq!(cmd.stdout, diout);
    assert_eq!(cmd.stderr, "".as_bytes());
}
