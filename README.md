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
FETCH_USING=            mktool
TOOLS_PLATFORM.mktool=  ${HOME}/.cargo/bin/mktool
```

However there is also a `pkgtools/mktool` package if you prefer to use that.

You will also need to apply changes to pkgsrc.  The easiest method is to just
use the [TritonDataCenter/pkgsrc](https://github.com/TritonDataCenter/pkgsrc)
fork as your primary repository, where the `trunk` branch includes the mktool
patch, as well as a few other well-tested improvements to pkgsrc, and is synced
with upstream [NetBSD/pkgsrc](https://github.com/NetBSD/pkgsrc) every hour.

```shell
$ git clone https://github.com/TritonDataCenter/pkgsrc

$ cd pkgsrc
< .. do usual pkgsrc things .. >

# Fetch the latest updates every hour
$ git pull
```

Alternatively the changes are in the
[feature/mktool/trunk](https://github.com/NetBSD/pkgsrc/compare/trunk...TritonDataCenter:pkgsrc:feature/mktool/trunk)
branch, and you can get them all as a single patch file
[here](https://github.com/NetBSD/pkgsrc/compare/trunk...TritonDataCenter:pkgsrc:feature/mktool/trunk.patch).

See the FAQ below for why this is not yet committed.

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

All of the replacements are activated upon setting `TOOLS_PLATFORM.mktool`,
with the exception of `fetch` which also requires `FETCH_USING=mktool` in case
the user has specific requirements for their fetch program.

In addition, packages no longer have build dependencies on `pkgtools/digest`
and `pkgtools/mktools` (unless specifically requested), which provides a
reasonable boost to performance in bulk builds.

## FAQ

### Why Have The Patches Not Been Merged Into pkgsrc?

I am uncomfortable committing changes that other developers have objections to.

While the overwhelming response has been positive, there are still concerns
from some that introducing mktool support will eventually lead to a mandatory
requirement on Rust in the future. I have explicitly stated (and have spent
over a decade proving by my actions) that this is not going to happen, and I
hope that one day people will realise this and the patches can be committed.

### Will This Ever Be Mandatory?

No.  pkgsrc supports over 20 operating systems, and on NetBSD alone 16
different CPU architectures.  I've spent over a decade as the primary advocate
for portability in pkgsrc.  Rust will never support all of those systems, so
the default will always be the portable shell and awk scripts.  I would be the
first person to reject any move towards a non-portable pkgsrc.

### What Is The Minimum Support Rust Version?

Currently [1.74.0](https://blog.rust-lang.org/2023/11/16/Rust-1.74.0.html).

The `edition` is set to 2021, so in theory I'd like to have 1.56.0 as the
MSRV, but newer releases are currently required due to `clap` and `tokio`
dependency requirements.
