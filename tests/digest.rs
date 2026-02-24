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

use std::io::Write;
use std::process::Command;

const MKTOOL: &str = env!("CARGO_BIN_EXE_mktool");

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/*
 * Verify that a valid file produces a correct hash on stdout and exits 0.
 */
#[test]
fn digest_valid_file() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("test.txt");
    std::fs::write(&file, b"hello\n")?;

    let cmd =
        Command::new(MKTOOL).args(["digest", "SHA256"]).arg(&file).output()?;

    let stdout = String::from_utf8_lossy(&cmd.stdout);
    let stderr = String::from_utf8_lossy(&cmd.stderr);

    assert!(cmd.status.success(), "expected success: {stderr}");
    assert!(
        stdout.contains("SHA256") && stdout.contains("="),
        "expected hash output: {stdout}"
    );
    assert!(stderr.is_empty(), "expected no stderr: {stderr}");
    Ok(())
}

/*
 * Verify that a missing file produces an error message on stderr that
 * includes the reason for the failure, and exits 1.
 */
#[test]
fn digest_missing_file() -> Result<()> {
    let cmd = Command::new(MKTOOL)
        .args(["digest", "SHA256", "/nonexistent/file.txt"])
        .output()?;

    let stderr = String::from_utf8_lossy(&cmd.stderr);

    assert!(!cmd.status.success(), "expected failure: {stderr}");
    assert_eq!(cmd.status.code(), Some(1));
    assert!(
        stderr.contains("/nonexistent/file.txt"),
        "expected filename in error: {stderr}"
    );
    assert!(
        stderr.contains("No such file") || stderr.contains("not found"),
        "expected error reason in output: {stderr}"
    );
    Ok(())
}

/*
 * Verify that a mix of valid and missing files reports all results, with
 * errors for the missing files and hashes for the valid ones.
 */
#[test]
fn digest_mixed_files() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let good = dir.path().join("good.txt");
    std::fs::write(&good, b"hello\n")?;

    let cmd = Command::new(MKTOOL)
        .args(["digest", "SHA256"])
        .arg(&good)
        .arg("/nonexistent/bad.txt")
        .output()?;

    let stdout = String::from_utf8_lossy(&cmd.stdout);
    let stderr = String::from_utf8_lossy(&cmd.stderr);

    assert!(!cmd.status.success(), "expected failure due to bad file");
    assert!(
        stdout.contains("good.txt"),
        "expected hash for good file: {stdout}"
    );
    assert!(
        stderr.contains("/nonexistent/bad.txt"),
        "expected error for bad file: {stderr}"
    );
    assert!(
        stderr.contains("No such file") || stderr.contains("not found"),
        "expected error reason: {stderr}"
    );
    Ok(())
}

/*
 * Verify that stdin mode works.
 */
#[test]
fn digest_stdin() -> Result<()> {
    let mut child = Command::new(MKTOOL)
        .args(["digest", "SHA256"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    child.stdin.take().ok_or("failed to open stdin")?.write_all(b"hello\n")?;

    let output = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "expected success");
    assert!(stdout.trim().len() == 64, "expected SHA256 hex string: {stdout}");
    Ok(())
}
