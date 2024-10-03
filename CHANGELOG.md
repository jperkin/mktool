## Version 1.3.2 (2024-10-03)

 * check-shlibs: Catch up with `USE_INDIRECT_DEPENDS` changes.

## Version 1.3.1 (2024-10-02)

 * check-shlibs: Fix issue running against pkgsrc trees that do not have the
   implicit library dependency checks.

## Version 1.3.0 (2024-10-01)

 * check-portability: Add new "mktool check-portability" command.  Runs 30x
   faster than the shell version in x11/qt5-qtwebengine on a MacBook Pro M1.
   Does not yet support `CHECK_PORTABILITY_EXPERIMENTAL=yes`.

## Version 1.2.0 (2024-09-20)

 * check-shlibs: Add new "mktool check-shlibs" command.  Should behave the
   same as the awk implementations, but with additional features, and much
   faster runtime (0.6s vs 20.4s for x11/kde-workspace4).

 * fetch: Avoid unnecessary re-fetching when running 'make makedistinfo'.

 * Minor cleanups to checksum, distinfo, and fetch.

## Version 1.1.0 (2024-09-13)

 * digest: Add new "mktool digest" command.  Aims for compatibility with
   pkgtools/digest, and should already serve as a drop-in replacement.

 * Update pkgsrc-rs and simplify some code accordingly.

 * Add note on Minimum Rust Supported Version (1.74.0).

## Version 1.0.1 (2024-09-13)

 * fetch: Use a shared client instance, improves performance quite a bit.

 * fetch: Disable Referer header, this appears to cause problems when trying
   to download through multiple redirects from SourceForge.

 * fetch: Minor updates to progress bar formatting, improves initial display
   while waiting for redirects to be followed.

 * fetch: Fix 'make makedistinfo' when there is no existing distinfo file.

## Version 1.0.0 (2024-09-12)

First official release, where mktool has been tested in bulk builds and other
real-world testing, and shown to cause no known regressions.
