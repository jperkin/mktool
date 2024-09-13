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
