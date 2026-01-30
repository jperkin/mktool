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
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const MKTOOL: &str = env!("CARGO_BIN_EXE_mktool");

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("failed to bind")
        .local_addr()
        .expect("failed to get local addr")
        .port()
}

fn start_nc(port: u16, initial_response: &str) -> Child {
    let child = Command::new("sh")
        .args([
            "-c",
            &format!(
                "(printf '{initial_response}'; cat) | nc -l 127.0.0.1 {port}"
            ),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start nc");
    thread::sleep(Duration::from_millis(200));
    child
}

fn has_temp_files(dir: &Path) -> bool {
    fs::read_dir(dir).expect("failed to read directory").any(|e| {
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
 *
 * Uses the non-"-" site prefix format so that the filename is appended
 * to the site URL, exercising the url_from_site() non-direct path.
 */
#[test]
fn fetch_https_http2() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let distdir = dir.path().to_str().expect("invalid tempdir path");

    let input = format!("robots.txt {distdir} https://www.google.com\n");

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
 * Verify FTP fetch works, reading input from a file rather than stdin.
 * Uses a subdirectory in the filepath to exercise create_dir_all().
 */
#[test]
fn fetch_ftp() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let distdir = dir.path().to_str().expect("invalid tempdir path");

    let input_file = dir.path().join("input");
    fs::write(
        &input_file,
        format!("sub/robots.txt {distdir} -ftp://ftp.netbsd.org/robots.txt\n"),
    )
    .expect("failed to write input file");

    let output = Command::new(MKTOOL)
        .args([
            "fetch",
            "-d",
            distdir,
            "-I",
            input_file.to_str().expect("invalid input path"),
        ])
        .env("MKTOOL_JOBS", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to run mktool fetch");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "fetch failed: {stderr}");
    assert!(
        dir.path().join("sub").join("robots.txt").exists(),
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

/*
 * Verify that a stalled FTP server triggers a read timeout and does not
 * hang indefinitely.  The nc listener sends the FTP greeting but never
 * responds to any commands.
 */
#[test]
fn fetch_ftp_read_timeout() {
    let port = free_port();
    let mut nc = start_nc(port, "220 ready\\r\\n");

    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let distdir = dir.path().to_str().expect("invalid tempdir path");
    let input =
        format!("test.txt {distdir} -ftp://127.0.0.1:{port}/test.txt\n");

    let start = Instant::now();

    let mut child = Command::new(MKTOOL)
        .args(["fetch", "-d", distdir, "-I", "-"])
        .env("MKTOOL_JOBS", "1")
        .env("MKTOOL_READ_TIMEOUT", "2")
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
    let elapsed = start.elapsed();

    let _ = nc.kill();
    let _ = nc.wait();

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "fetch should have failed: {stderr}");
    assert!(
        elapsed.as_secs() < 30,
        "read timeout did not fire, took {elapsed:?}"
    );
    assert!(!has_temp_files(dir.path()), "temp file not cleaned up");
}

/*
 * Verify that invalid input (too few fields) is handled gracefully.
 */
#[test]
fn fetch_invalid_input() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let distdir = dir.path().to_str().expect("invalid tempdir path");

    let mut child = Command::new(MKTOOL)
        .args(["fetch", "-d", distdir, "-j", "1", "-I", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run mktool fetch");

    child
        .stdin
        .take()
        .expect("failed to open stdin")
        .write_all(b"onlyonefield\n")
        .expect("failed to write to stdin");

    let output = child.wait_with_output().expect("failed to wait on child");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "fetch should have failed: {stderr}");
    assert!(
        stderr.contains("Invalid input"),
        "expected invalid input error: {stderr}"
    );
}

/*
 * Verify that an HTTP 404 response is handled gracefully.
 */
#[test]
fn fetch_http_404() {
    let port = free_port();
    let mut nc = start_nc(
        port,
        "HTTP/1.1 404 Not Found\\r\\nContent-Length: 0\\r\\n\\r\\n",
    );

    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let distdir = dir.path().to_str().expect("invalid tempdir path");
    let input =
        format!("test.txt {distdir} -http://127.0.0.1:{port}/test.txt\n");

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

    let _ = nc.kill();
    let _ = nc.wait();

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "fetch should have failed: {stderr}");
    assert!(stderr.contains("404"), "expected 404 error in output: {stderr}");
    assert!(
        !dir.path().join("test.txt").exists(),
        "no file should have been created"
    );
    assert!(!has_temp_files(dir.path()), "temp file not cleaned up");
}

/*
 * Verify that re-fetching an already-downloaded file is a no-op.
 */
#[test]
fn fetch_https_refetch() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let distdir = dir.path().to_str().expect("invalid tempdir path");

    let input =
        format!("robots.txt {distdir} -https://www.google.com/robots.txt\n");

    for _ in 0..2 {
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
    }

    assert!(
        dir.path().join("robots.txt").exists(),
        "downloaded file not found"
    );
    assert!(!has_temp_files(dir.path()), "temp file not cleaned up");
}

/*
 * Verify that an HTTP connection error (nothing listening) is handled
 * gracefully.
 */
#[test]
fn fetch_http_connect_error() {
    let port = free_port();

    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let distdir = dir.path().to_str().expect("invalid tempdir path");
    let input =
        format!("test.txt {distdir} -http://127.0.0.1:{port}/test.txt\n");

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

    assert!(!output.status.success(), "fetch should have failed: {stderr}");
    assert!(
        stderr.contains("Unable to fetch"),
        "expected connection error: {stderr}"
    );
    assert!(
        !dir.path().join("test.txt").exists(),
        "no file should have been created"
    );
    assert!(!has_temp_files(dir.path()), "temp file not cleaned up");
}
