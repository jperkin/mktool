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

mod check_shlibs;
mod checksum;
mod ctfconvert;
mod digest;
mod distinfo;
mod fetch;
mod symlinks;

const MKTOOL_DEFAULT_THREADS: usize = 4;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
#[command(rename_all = "lower")]
enum Commands {
    /// Perform shared library checks
    #[command(name = "check-shlibs")]
    CheckShlibs(check_shlibs::CheckShlibs),
    /// Verify checksums from a distinfo file.
    CheckSum(checksum::CheckSum),
    /// Convert DWARF debug information in binary files to CTF.
    CTFConvert(ctfconvert::CTFConvert),
    /// Calculate file digests
    Digest(digest::DigestCmd),
    /// Create or update distinfo file.
    DistInfo(distinfo::DistInfo),
    /// Fetch distfiles.
    Fetch(fetch::Fetch),
    /// Create symlinks.
    Symlinks(symlinks::Symlinks),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let rv = match &cli.command {
        Commands::CheckShlibs(cmd) => cmd.run()?,
        Commands::CheckSum(cmd) => cmd.run()?,
        Commands::CTFConvert(cmd) => cmd.run()?,
        Commands::Digest(cmd) => cmd.run()?,
        Commands::DistInfo(cmd) => cmd.run()?,
        Commands::Fetch(cmd) => cmd.run()?,
        Commands::Symlinks(cmd) => cmd.run()?,
    };

    std::process::exit(rv);
}
