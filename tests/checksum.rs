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
use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_checksum() {
    let mktool = env!("CARGO_BIN_EXE_mktool");
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");

    /*
     * Invalid arguments.
     *
     * XXX: This is not fully compatible, checksum.awk exits 3, we exit 2, but
     * unfortunatly this is hardcoded in clap.
     */
    let cmd = Command::new(mktool)
        .arg("checksum")
        .output()
        .expect("unable to spawn {mktool}");
    assert_eq!(cmd.status.code(), Some(2));

    /*
     * Test running with no input files.
     */
    let cmd = Command::new(mktool)
        .arg("checksum")
        .arg(distinfo.clone())
        .output()
        .expect("unable to spawn {mktool}");
    assert_eq!(cmd.status.code(), Some(0));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());

    /*
     * Pass a nonexistent distinfo file.
     */
    let mut bad_di = distinfo.clone();
    bad_di.push("oops");
    let output =
        format!("checksum: distinfo file missing: {}\n", bad_di.display());
    let cmd = Command::new(mktool)
        .arg("checksum")
        .arg(bad_di)
        .output()
        .expect("unable to spawn {mktool}");
    assert_eq!(cmd.status.code(), Some(3));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert_eq!(cmd.stderr, output.as_bytes());

    /*
     * Passing a file that doesn't exist in distinfo should print a message
     * to stderr and exit 2.
     */
    let cmd = Command::new(mktool)
        .arg("checksum")
        .arg(distinfo.clone())
        .arg("foo")
        .output()
        .expect("unable to spawn {mktool}");
    assert_eq!(cmd.status.code(), Some(2));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert_eq!(
        cmd.stderr,
        "checksum: No checksum recorded for foo\n".as_bytes()
    );

    /*
     * A valid full distfile test.
     */
    let mut output = String::from("=> Checksum BLAKE2s OK for digest1.txt\n");
    output.push_str("=> Checksum SHA512 OK for digest1.txt\n");
    let cmd = Command::new(mktool)
        .arg("checksum")
        .arg(distinfo.clone())
        .arg("digest1.txt")
        .current_dir("tests/data")
        .output()
        .expect("unable to spawn {mktool}");
    assert_eq!(cmd.status.code(), Some(0));
    assert_eq!(cmd.stdout, output.as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());

    /*
     * Valid, but we only request a single algorithm each time.
     */
    let output = "=> Checksum BLAKE2s OK for digest1.txt\n";
    let cmd = Command::new(mktool)
        .arg("checksum")
        .arg("-a")
        .arg("BLAKE2s")
        .arg(distinfo.clone())
        .arg("digest1.txt")
        .current_dir("tests/data")
        .output()
        .expect("unable to spawn {mktool}");
    assert_eq!(cmd.status.code(), Some(0));
    assert_eq!(cmd.stdout, output.as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());

    let output = "=> Checksum SHA512 OK for digest2.txt\n";
    let cmd = Command::new(mktool)
        .arg("checksum")
        .arg("-a")
        .arg("SHA512")
        .arg(distinfo.clone())
        .arg("digest2.txt")
        .current_dir("tests/data")
        .output()
        .expect("unable to spawn {mktool}");
    assert_eq!(cmd.status.code(), Some(0));
    assert_eq!(cmd.stdout, output.as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());

    /*
     * No checksum type recorded.
     */
    let output = "checksum: No SHA1 checksum recorded for digest1.txt\n";
    let cmd = Command::new(mktool)
        .arg("checksum")
        .arg("-a")
        .arg("SHA1")
        .arg(distinfo.clone())
        .arg("digest1.txt")
        .current_dir("tests/data")
        .output()
        .expect("unable to spawn {mktool}");
    assert_eq!(cmd.status.code(), Some(2));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert_eq!(cmd.stderr, output.as_bytes());

    /*
     * Patch mode.
     */
    let output = "=> Checksum SHA1 OK for patch-Makefile\n";
    let cmd = Command::new(mktool)
        .arg("checksum")
        .arg("-p")
        .arg(distinfo.clone())
        .arg("patch-Makefile")
        .current_dir("tests/data")
        .output()
        .expect("unable to spawn {mktool}");
    dbg!(&cmd);
    assert_eq!(cmd.status.code(), Some(0));
    assert_eq!(cmd.stdout, output.as_bytes());
    assert_eq!(cmd.stderr, "".as_bytes());

    /*
     * Now run some of the same tests but with a bad distinfo file.
     */
    let mut bad_di = distinfo.clone();
    bad_di.set_file_name("distinfo.bad");

    // Only the first checksum mismatch is printed on failure.
    let output = "checksum: Checksum BLAKE2s mismatch for digest1.txt\n";
    let cmd = Command::new(mktool)
        .arg("checksum")
        .arg(bad_di.clone())
        .arg("digest1.txt")
        .current_dir("tests/data")
        .output()
        .expect("unable to spawn {mktool}");
    assert_eq!(cmd.status.code(), Some(1));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert_eq!(cmd.stderr, output.as_bytes());

    let output = "checksum: Checksum SHA512 mismatch for digest1.txt\n";
    let cmd = Command::new(mktool)
        .arg("checksum")
        .arg("-a")
        .arg("SHA512")
        .arg(bad_di.clone())
        .arg("digest1.txt")
        .current_dir("tests/data")
        .output()
        .expect("unable to spawn {mktool}");
    assert_eq!(cmd.status.code(), Some(1));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert_eq!(cmd.stderr, output.as_bytes());

    let output = "checksum: Checksum SHA1 mismatch for patch-Makefile\n";
    let cmd = Command::new(mktool)
        .arg("checksum")
        .arg("-p")
        .arg(bad_di.clone())
        .arg("patch-Makefile")
        .current_dir("tests/data")
        .output()
        .expect("unable to spawn {mktool}");
    assert_eq!(cmd.status.code(), Some(1));
    assert_eq!(cmd.stdout, "".as_bytes());
    assert_eq!(cmd.stderr, output.as_bytes());
}
