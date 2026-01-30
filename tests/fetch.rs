/*
 * Copyright (c) 2026 Jonathan Perkin <jonathan@perkin.org.uk>
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

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const MKTOOL: &str = env!("CARGO_BIN_EXE_mktool");

fn has_temp_files(dir: &Path) -> bool {
    fs::read_dir(dir)
        .expect("failed to read directory")
        .any(|e| {
            e.expect("failed to read dir entry")
                .file_name()
                .to_string_lossy()
                .contains(".mktool.")
        })
}

/*
 * Verify that fetching from an HTTPS server that negotiates HTTP/2 via
 * ALPN does not panic.  This catches the case where the http2 feature is
 * missing from the reqwest dependency, which causes hyper-util to panic
 * at runtime when the TLS layer negotiates h2.
 */
#[test]
fn fetch_https_http2() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let distdir = dir.path().to_str().expect("invalid tempdir path");

    /*
     * Input format is "filepath distdir site [site ...]".  A site
     * prefixed with "-" is used as the complete URL.
     */
    let input =
        format!("robots.txt {distdir} -https://www.google.com/robots.txt\n");

    let mut child = Command::new(MKTOOL)
        .args(["fetch", "-d", distdir, "-I", "-"])
        .env("MKTOOL_JOBS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run mktool fetch");

    child
        .stdin
        .take()
        .expect("failed to open stdin")
        .write_all(input.as_bytes())
        .expect("failed to write to stdin");

    let output = child.wait_with_output().expect("failed to wait on child");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "fetch failed: {stderr}");
    assert!(
        dir.path().join("robots.txt").exists(),
        "downloaded file not found"
    );
    assert!(!has_temp_files(dir.path()), "temp file not cleaned up");
}

#[test]
fn fetch_ftp() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let distdir = dir.path().to_str().expect("invalid tempdir path");

    let input =
        format!("robots.txt {distdir} -ftp://ftp.netbsd.org/robots.txt\n");

    let mut child = Command::new(MKTOOL)
        .args(["fetch", "-d", distdir, "-I", "-"])
        .env("MKTOOL_JOBS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run mktool fetch");

    child
        .stdin
        .take()
        .expect("failed to open stdin")
        .write_all(input.as_bytes())
        .expect("failed to write to stdin");

    let output = child.wait_with_output().expect("failed to wait on child");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "fetch failed: {stderr}");
    assert!(
        dir.path().join("robots.txt").exists(),
        "downloaded file not found"
    );
    assert!(!has_temp_files(dir.path()), "temp file not cleaned up");
}

/*
 * Verify that a checksum mismatch causes the fetch to fail and that no
 * partial or incorrect file is left behind.
 */
#[test]
fn fetch_https_bad_checksum() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let distdir = dir.path().to_str().expect("invalid tempdir path");

    let distinfo = dir.path().join("distinfo");
    fs::write(
        &distinfo,
        "BLAKE2s (robots.txt) = 0000000000000000000000000000000000000000000000000000000000000000\n",
    )
    .expect("failed to write distinfo");

    let input =
        format!("robots.txt {distdir} -https://www.google.com/robots.txt\n");

    let mut child = Command::new(MKTOOL)
        .args([
            "fetch",
            "-d",
            distdir,
            "-f",
            distinfo.to_str().expect("invalid distinfo path"),
            "-I",
            "-",
        ])
        .env("MKTOOL_JOBS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run mktool fetch");

    child
        .stdin
        .take()
        .expect("failed to open stdin")
        .write_all(input.as_bytes())
        .expect("failed to write to stdin");

    let output = child.wait_with_output().expect("failed to wait on child");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "fetch should have failed: {stderr}");
    assert!(
        stderr.contains("Verification failed"),
        "expected checksum verification failure: {stderr}"
    );
    assert!(
        !dir.path().join("robots.txt").exists(),
        "failed file should have been cleaned up"
    );
    assert!(!has_temp_files(dir.path()), "temp file not cleaned up");
}

#[test]
fn fetch_ftp_bad_checksum() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let distdir = dir.path().to_str().expect("invalid tempdir path");

    let distinfo = dir.path().join("distinfo");
    fs::write(
        &distinfo,
        "BLAKE2s (robots.txt) = 0000000000000000000000000000000000000000000000000000000000000000\n",
    )
    .expect("failed to write distinfo");

    let input =
        format!("robots.txt {distdir} -ftp://ftp.netbsd.org/robots.txt\n");

    let mut child = Command::new(MKTOOL)
        .args([
            "fetch",
            "-d",
            distdir,
            "-f",
            distinfo.to_str().expect("invalid distinfo path"),
            "-I",
            "-",
        ])
        .env("MKTOOL_JOBS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run mktool fetch");

    child
        .stdin
        .take()
        .expect("failed to open stdin")
        .write_all(input.as_bytes())
        .expect("failed to write to stdin");

    let output = child.wait_with_output().expect("failed to wait on child");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "fetch should have failed: {stderr}");
    assert!(
        stderr.contains("Verification failed"),
        "expected checksum verification failure: {stderr}"
    );
    assert!(
        !dir.path().join("robots.txt").exists(),
        "failed file should have been cleaned up"
    );
    assert!(!has_temp_files(dir.path()), "temp file not cleaned up");
}
