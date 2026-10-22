/*
 * Copyright (c) 2026 Jonathan Perkin <jonathan@perkin.org.uk>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE. IN NO EVENT SHALL
 * THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL
 * DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR
 * PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS
 * ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
 * THIS SOFTWARE.
 */

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::tempdir;

fn run_check_portability(
    testdir: &Path,
    input: &[u8],
    patchdir: Option<&Path>,
    skipglob: Option<&str>,
) -> Result<Output, Box<dyn std::error::Error>> {
    fs::write(testdir.join("script"), input)?;

    let mut command = Command::new(env!("CARGO_BIN_EXE_mktool"));
    command
        .arg("check-portability")
        .current_dir(testdir)
        .env_remove("CHECK_PORTABILITY_SKIP")
        .env_remove("PATCHDIR");
    if let Some(patchdir) = patchdir {
        command.env("PATCHDIR", patchdir);
    }
    if let Some(skipglob) = skipglob {
        command.env("CHECK_PORTABILITY_SKIP", skipglob);
    }
    Ok(command.output()?)
}

#[test]
fn detects_violations_on_utf8_lines() -> Result<(), Box<dyn std::error::Error>>
{
    let tmpdir = tempdir()?;
    let output = run_check_portability(
        tmpdir.path(),
        "#!/bin/sh\nif [ café == bar ]; then\nprintf '$RANDOM'\n".as_bytes(),
        None,
        None,
    )?;

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("script:2: if [ café == bar ]; then"));
    assert!(stderr.contains("script:3: printf '$RANDOM'"));
    Ok(())
}

#[test]
fn ignores_safe_utf8_lines() -> Result<(), Box<dyn std::error::Error>> {
    let tmpdir = tempdir()?;
    let output = run_check_portability(
        tmpdir.path(),
        "#!/bin/sh\n# café $RANDOM\n   # $RANDOM\nif [ café = bar ]; then\necho ok\n".as_bytes(),
        None,
        None,
    )?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn ignores_safe_non_utf8_lines() -> Result<(), Box<dyn std::error::Error>> {
    let tmpdir = tempdir()?;
    let output = run_check_portability(
        tmpdir.path(),
        b"#!/bin/sh\n# \xff $RANDOM\n   # \xfe $RANDOM\nprintf '\xfe'\n",
        None,
        None,
    )?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn detects_violations_on_non_utf8_lines()
-> Result<(), Box<dyn std::error::Error>> {
    let tmpdir = tempdir()?;
    let output = run_check_portability(
        tmpdir.path(),
        b"#!/bin/sh\nif [ foo == bar \xff ]; then\necho \xfe $RANDOM\n",
        None,
        None,
    )?;

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("script:2:"), "unexpected stderr: {stderr}");
    assert!(stderr.contains("script:3:"), "unexpected stderr: {stderr}");
    assert!(stderr.contains('\u{fffd}'));
    Ok(())
}

#[test]
fn handles_non_utf8_patch_input() -> Result<(), Box<dyn std::error::Error>> {
    let tmpdir = tempdir()?;
    let patchdir = tmpdir.path().join("patches");
    fs::create_dir(&patchdir)?;
    fs::write(patchdir.join("not-a-patch"), b"patch description \xff\n")?;
    fs::write(
        patchdir.join("patch-test"),
        b"patch description \xff\n+++ script\n",
    )?;
    fs::write(
        tmpdir.path().join("script.in"),
        b"#!/bin/sh\nif [ foo == bar ]; then\n",
    )?;

    let output = run_check_portability(
        tmpdir.path(),
        b"not a shell script\n",
        Some(&patchdir),
        None,
    )?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn skips_patched_template_with_crlf() -> Result<(), Box<dyn std::error::Error>>
{
    check_patch_template_skip(b"+++ script\r\n")
}

#[test]
fn skips_patched_template_with_non_utf8_crlf_patch()
-> Result<(), Box<dyn std::error::Error>> {
    check_patch_template_skip(b"patch description \xff\r\n+++ script\r\n")
}

#[test]
fn skips_patched_template_with_cr_at_eof()
-> Result<(), Box<dyn std::error::Error>> {
    check_patch_template_skip(b"+++ script\r")
}

fn check_patch_template_skip(
    patch: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let tmpdir = tempdir()?;
    let patchdir = tmpdir.path().join("patches");
    fs::create_dir(&patchdir)?;
    fs::write(patchdir.join("patch-test"), patch)?;
    fs::write(
        tmpdir.path().join("script.in"),
        b"#!/bin/sh\nif [ foo == bar ]; then\n",
    )?;

    let output = run_check_portability(
        tmpdir.path(),
        b"not a shell script\n",
        Some(&patchdir),
        None,
    )?;
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn honors_skip_glob() -> Result<(), Box<dyn std::error::Error>> {
    let tmpdir = tempdir()?;
    let output = run_check_portability(
        tmpdir.path(),
        b"#!/bin/sh\nif [ foo == bar ]; then\n",
        None,
        Some("script"),
    )?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn warns_on_invalid_skip_glob() -> Result<(), Box<dyn std::error::Error>> {
    let tmpdir = tempdir()?;
    let output = run_check_portability(
        tmpdir.path(),
        b"not a shell script\n",
        None,
        Some("["),
    )?;

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("WARNING: invalid CHECK_PORTABILITY_SKIP glob"));
    Ok(())
}

#[test]
fn skips_files_by_extension() -> Result<(), Box<dyn std::error::Error>> {
    let tmpdir = tempdir()?;
    fs::write(
        tmpdir.path().join("ignored.txt"),
        b"#!/bin/sh\nif [ foo == bar ]; then\n",
    )?;
    let output = run_check_portability(
        tmpdir.path(),
        b"not a shell script\n",
        None,
        None,
    )?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn ignores_incomplete_hashbang() -> Result<(), Box<dyn std::error::Error>> {
    let tmpdir = tempdir()?;
    let output =
        run_check_portability(tmpdir.path(), b"#!/bin/sh", None, None)?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn ignores_non_sh_hashbang() -> Result<(), Box<dyn std::error::Error>> {
    let tmpdir = tempdir()?;
    let output = run_check_portability(
        tmpdir.path(),
        b"#!/bin/bash\nif [ foo == bar ]; then\n",
        None,
        None,
    )?;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    Ok(())
}
