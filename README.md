# mktool

This is a collection of utilities that provide alternate implementations for
parts of the [pkgsrc](https://github.com/NetBSD/pkgsrc/) mk infrastructure.

Many targets under `pkgsrc/mk` are implemented using a combination of shell
and awk, and can suffer from a lack of performance, especially when input
sizes grow.

For example, with the profligation of Go modules used in newer Go software,
`www/grafana` now has over 5,000 distfiles.  This exposes various issues in
the current pkgsrc scripts that are difficult to work around.  This tool
implements replacements with the following performance improvements when
running in `www/grafana` on a 32-core SmartOS host:

|          Command | Existing pkgsrc scripts |      mktool |  Speedup |
|-----------------:|------------------------:|------------:|---------:|
| `bmake checksum` |              10 seconds |   2 seconds |   **5x** |
| `bmake distinfo` |   3 minutes, 30 seconds |   2 seconds | **100x** |
|    `bmake fetch` |  47 minutes, 58 seconds |   5 seconds | **500x** |

As pkgsrc strives to be as portable as possible, at no point will any of the
commands implemented by `mktool` become mandatory.  This tool simply exists
for those who are able to run Rust software to dramatically improve pkgsrc
performance.

## Installation

Install using `cargo`:

```shell
$ cargo install mktool
```

and add the following to `mk.conf`:

```make
TOOLS_PLATFORM.mktool=  ${HOME}/.cargo/bin/mktool
```

You will also need to apply changes to pkgsrc.  The changes are in the
[dev/mktool](https://github.com/NetBSD/pkgsrc/compare/trunk...TritonDataCenter:pkgsrc:dev/mktool)
branch, and you can get them all as a single patch file
[here](https://github.com/NetBSD/pkgsrc/compare/trunk...TritonDataCenter:pkgsrc:dev/mktool.patch).

## Commands

These are the commands currently implemented:

|      Command | Replaces |
|-------------:|---------:|
|   `checksum` | [mk/checksum/checksum.awk](https://github.com/NetBSD/pkgsrc/blob/trunk/mk/checksum/checksum.awk) |
| `ctfconvert` | [mk/install/install.mk:install-ctf](https://github.com/NetBSD/pkgsrc/blob/1660a054/mk/install/install.mk#L357-L384) |
|   `distinfo` | [mk/checksum/distinfo.awk](https://github.com/NetBSD/pkgsrc/blob/trunk/mk/checksum/distinfo.awk) |
|      `fetch` | [mk/fetch/fetch](https://github.com/NetBSD/pkgsrc/blob/trunk/mk/fetch/fetch) |
|   `symlinks` | [pkgtools/mktools](https://github.com/NetBSD/pkgsrc/blob/trunk/pkgtools/mktools/files/mk-buildlink-symlinks.c) |

All of the replacements are activated upon setting `TOOLS_PLATFORM.mktool`.
In addition, packages no longer have build dependencies on `pkgtools/digest`
and `pkgtools/mktools` (unless specifically requested).
