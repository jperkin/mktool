#![cfg(target_os = "macos")]
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
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const MKTOOL: &str = env!("CARGO_BIN_EXE_mktool");

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/*
 * Mach-O constants from <mach-o/loader.h>.
 */
const MH_MAGIC_64: u32 = 0xFEED_FACF;
const CPU_TYPE_X86_64: u32 = 0x0100_0007;
const MH_EXECUTE: u32 = 2;
const MH_DYLIB: u32 = 6;
const LC_LOAD_DYLIB: u32 = 0x0C;
const LC_ID_DYLIB: u32 = 0x0D;

/*
 * Size of the fixed part of `struct dylib_command` (six u32 fields); the
 * variable-length library name follows.
 */
const DYLIB_CMD_HEADER_SIZE: u32 = 24;

fn push_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

/*
 * Build a dylib load command (LC_ID_DYLIB or LC_LOAD_DYLIB).  The library
 * name is null-terminated and the whole command is padded to 8-byte
 * alignment, as required by the Mach-O format.
 */
fn build_dylib_cmd(cmd_type: u32, name: &str) -> Vec<u8> {
    let name_bytes = name.as_bytes();
    let cmd_size = (DYLIB_CMD_HEADER_SIZE as usize + name_bytes.len() + 1)
        .next_multiple_of(8);

    let mut buf = Vec::with_capacity(cmd_size);
    push_u32(&mut buf, cmd_type);
    push_u32(&mut buf, cmd_size as u32);
    push_u32(&mut buf, DYLIB_CMD_HEADER_SIZE); /* name offset */
    push_u32(&mut buf, 0); /* timestamp */
    push_u32(&mut buf, 0); /* current_version */
    push_u32(&mut buf, 0); /* compatibility_version */
    buf.extend_from_slice(name_bytes);
    buf.push(0);
    buf.resize(cmd_size, 0);
    buf
}

/*
 * Build a minimal Mach-O 64-bit binary that goblin can parse.  Contains
 * only a mach_header_64 and dylib load commands, no segments needed.
 */
fn build_macho(
    filetype: u32,
    id_dylib: Option<&str>,
    deps: &[&str],
) -> Vec<u8> {
    let mut cmds: Vec<u8> = Vec::new();
    let mut ncmds: u32 = 0;

    if let Some(name) = id_dylib {
        cmds.extend(build_dylib_cmd(LC_ID_DYLIB, name));
        ncmds += 1;
    }
    for dep in deps {
        cmds.extend(build_dylib_cmd(LC_LOAD_DYLIB, dep));
        ncmds += 1;
    }

    let mut macho: Vec<u8> = Vec::new();
    push_u32(&mut macho, MH_MAGIC_64);
    push_u32(&mut macho, CPU_TYPE_X86_64);
    push_u32(&mut macho, 0); /* cpusubtype */
    push_u32(&mut macho, filetype);
    push_u32(&mut macho, ncmds);
    push_u32(&mut macho, cmds.len() as u32); /* sizeofcmds */
    push_u32(&mut macho, 0); /* flags */
    push_u32(&mut macho, 0); /* reserved */
    macho.extend(cmds);
    macho
}

fn run_check_shlibs(
    testdir: &Path,
    input: &str,
    extra_env: &[(&str, &str)],
) -> Result<Output> {
    let destdir = testdir.join("destdir");
    let wrkdir = testdir.join("wrkdir");
    let depends = testdir.join("depends");
    fs::create_dir_all(&destdir)?;
    fs::create_dir_all(&wrkdir)?;
    fs::write(&depends, "")?;

    let mut builder = Command::new(MKTOOL);
    builder
        .arg("check-shlibs")
        .env("DESTDIR", &destdir)
        .env("CROSS_DESTDIR", "")
        .env("WRKDIR", &wrkdir)
        .env("PKG_ADMIN_CMD", "/usr/bin/true")
        .env("DEPENDS_FILE", &depends)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in extra_env {
        builder.env(k, v);
    }
    let mut cmd = builder.spawn()?;

    let mut stdin = cmd.stdin.take().ok_or("failed to open stdin")?;
    stdin.write_all(input.as_bytes())?;
    drop(stdin);
    Ok(cmd.wait_with_output()?)
}

/*
 * For shared libraries, libs[0] is the LC_ID_DYLIB install name which
 * should be verified alongside regular dependencies.
 */
#[test]
fn test_dylib_includes_install_name() -> Result<()> {
    let tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let testdir = tmpdir.join("check_shlibs_dylib");
    fs::create_dir_all(&testdir)?;

    let libfoo = testdir.join("lib/libfoo.dylib");
    let libbar = testdir.join("lib/libbar.dylib");
    let libfoo_str = libfoo.to_str().ok_or("invalid path")?;
    let libbar_str = libbar.to_str().ok_or("invalid path")?;

    let macho = build_macho(MH_DYLIB, Some(libfoo_str), &[libbar_str]);
    let bin_path = testdir.join("test.dylib");
    fs::write(&bin_path, &macho)?;

    let out =
        run_check_shlibs(&testdir, &format!("{}\n", bin_path.display()), &[])?;

    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout)?;
    assert!(
        stdout.contains(&format!("missing library: {libfoo_str}")),
        "install name should be checked: {stdout}"
    );
    assert!(
        stdout.contains(&format!("missing library: {libbar_str}")),
        "dependency should be checked: {stdout}"
    );
    Ok(())
}

/*
 * For executables, libs[0] is a "self" placeholder from goblin that
 * should be skipped.
 */
#[test]
fn test_exe_skips_self() -> Result<()> {
    let tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let testdir = tmpdir.join("check_shlibs_exe");
    fs::create_dir_all(&testdir)?;

    let libbar = testdir.join("lib/libbar.dylib");
    let libbar_str = libbar.to_str().ok_or("invalid path")?;

    let macho = build_macho(MH_EXECUTE, None, &[libbar_str]);
    let bin_path = testdir.join("test_exe");
    fs::write(&bin_path, &macho)?;

    let out =
        run_check_shlibs(&testdir, &format!("{}\n", bin_path.display()), &[])?;

    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout)?;
    assert!(
        stdout.contains(&format!("missing library: {libbar_str}")),
        "dependency should be checked: {stdout}"
    );
    assert!(
        !stdout.contains("self"),
        "self placeholder should be skipped: {stdout}"
    );
    Ok(())
}

/*
 * A dependency staged under DESTDIR (the pre-install location) is
 * silently accepted: no further checks run and nothing is printed.
 */
#[test]
fn test_dep_under_destdir_passes() -> Result<()> {
    let tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let testdir = tmpdir.join("check_shlibs_destdir");
    fs::create_dir_all(&testdir)?;

    /*
     * The binary references the eventual install path; the file only
     * exists under DESTDIR at this point.  The absolute path is
     * arbitrary: check-shlibs strips the leading "/" and looks for the
     * remainder under DESTDIR.
     */
    let install_path = "/libfoo.dylib";
    let staged = testdir.join("destdir/libfoo.dylib");
    fs::write(&staged, b"")?;

    let macho = build_macho(MH_EXECUTE, None, &[install_path]);
    let bin_path = testdir.join("test_exe");
    fs::write(&bin_path, &macho)?;

    let out =
        run_check_shlibs(&testdir, &format!("{}\n", bin_path.display()), &[])?;

    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout)?;
    assert!(stdout.is_empty(), "expected no output: {stdout}");
    assert!(out.stderr.is_empty(), "expected no stderr");
    Ok(())
}

/*
 * A dependency that exists at its referenced absolute path but matches
 * CHECK_SHLIBS_TOXIC should be flagged.
 */
#[test]
fn test_toxic_dep_flagged() -> Result<()> {
    let tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let testdir = tmpdir.join("check_shlibs_toxic");
    fs::create_dir_all(&testdir)?;

    /*
     * The file must exist at the absolute path referenced so that the
     * direct-path branch runs check_shlib; otherwise we'd hit the
     * "missing library" fast path before the toxic check.
     */
    let libtoxic = testdir.join("libtoxic.dylib");
    fs::write(&libtoxic, b"")?;
    let libtoxic_str = libtoxic.to_str().ok_or("invalid path")?;

    let macho = build_macho(MH_EXECUTE, None, &[libtoxic_str]);
    let bin_path = testdir.join("test_exe");
    fs::write(&bin_path, &macho)?;

    let out = run_check_shlibs(
        &testdir,
        &format!("{}\n", bin_path.display()),
        &[("CHECK_SHLIBS_TOXIC", "libtoxic\\.dylib")],
    )?;

    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout)?;
    assert!(
        stdout.contains("matches toxic"),
        "toxic lib should be flagged: {stdout}"
    );
    Ok(())
}
