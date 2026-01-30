# mktool

This is a collection of utilities that provide alternate implementations for
parts of the [pkgsrc](https://github.com/NetBSD/pkgsrc/) mk infrastructure.

## Benchmarks

The main focus is on performance.  Here are some real-world numbers showing
improvements seen, compared to the baseline shell/awk implementations:

|  Script / Target | Without mktool | With mktool |  Speedup |
|------------------:|--------------:|------------:|---------:|
| check-portability |           35s |          1s |  **30x** |
|      check-shlibs |           20s |         <1s |  **30x** |
|          checksum |           10s |          2s |   **5x** |
|        ctfconvert |       40m 39s |      5m 13s |   **8x** |
|          distinfo |        3m 30s |          2s | **100x** |
|             fetch |       47m 58s |          5s | **500x** |
|           wrapper |        1m 41s |          9s |  **11x** |

## User Improvements

As well as superior performance, where possible mktool also aims to provide an
improved user experience.  For example the `fetch` replacement features a
significantly better progress bar and streamlined output compared to other
available fetch backends
([see terminal recording](https://asciinema.org/a/S4MWXHLSJmL4GKYAhOBIIHE31)).

## Installation

The preferred method of installation is using `cargo`:

```shell
$ cargo install mktool
```

and adding the following to `mk.conf`:

```make
TOOLS_PLATFORM.mktool=  ${HOME}/.cargo/bin/mktool
```

However there is also a `pkgtools/mktool` package if you prefer to use that.

The necessary changes to pkgsrc were committed on 2024-10-11.  If you are
running a pkgsrc older than this date and still want to use mktool then you
will have to apply
[this commit](https://github.com/NetBSD/pkgsrc/commit/a68ab6cb39f56b9e9b0025993d634455f416f267)
yourself manually.

## Commands

These are the commands currently implemented:

|             Command | Replaces |
|--------------------:|---------:|
| `check-portability` | [mk/check/check-portability.awk](https://github.com/NetBSD/pkgsrc/blob/trunk/mk/check/check-portability.awk) |
|      `check-shlibs` | [mk/check/check-shlibs-\*.awk](https://github.com/NetBSD/pkgsrc/blob/trunk/mk/check/check-shlibs-elf.awk) |
|          `checksum` | [mk/checksum/checksum.awk](https://github.com/NetBSD/pkgsrc/blob/trunk/mk/checksum/checksum.awk) |
|        `ctfconvert` | [mk/install/install.mk:install-ctf](https://github.com/NetBSD/pkgsrc/blob/1660a054/mk/install/install.mk#L357-L384) |
|            `digest` | [pkgtools/digest](https://github.com/NetBSD/pkgsrc/blob/trunk/pkgtools/digest/Makefile) |
|          `distinfo` | [mk/checksum/distinfo.awk](https://github.com/NetBSD/pkgsrc/blob/trunk/mk/checksum/distinfo.awk) |
|             `fetch` | [mk/fetch/fetch](https://github.com/NetBSD/pkgsrc/blob/trunk/mk/fetch/fetch) |
|          `symlinks` | [pkgtools/mktools](https://github.com/NetBSD/pkgsrc/blob/trunk/pkgtools/mktools/files/mk-buildlink-symlinks.c) |

All of the replacements are activated upon setting `TOOLS_PLATFORM.mktool`.

In addition, packages no longer have build dependencies on `pkgtools/digest`
and `pkgtools/mktools` (unless specifically requested), which provides a
reasonable boost to performance in bulk builds.

## FAQ

### Will This Ever Be Mandatory?

No.  pkgsrc supports over 20 operating systems, and on NetBSD alone 16
different CPU architectures.  I've spent over a decade as the primary advocate
for portability in pkgsrc.  Rust will never support all of those systems, so
the default will always be the portable shell and awk scripts.  I would be the
first person to reject any move towards a non-portable pkgsrc.

### What Is The Minimum Support Rust Version?

Currently [1.85.1](https://blog.rust-lang.org/2025/03/18/Rust-1.85.1/) for
minimum compatibility with the 2024 edition of Rust.

As this is an end-user application and not a library, the MSRV may be bumped a
little sooner than it would for e.g.
[pkgsrc-rs](https://crates.io/crates/pkgsrc), but will generally be kept back
as much as possible unless there are compelling reasons to update.
