# HyperSearch 0.3 beta ? Windows x64

Portable beta: download HyperSearch.exe, or unpack the ZIP and run it. Personal indexes, caches and settings are not included.

## Changes
- Blue Explorer-inspired layout with compact native file icons and a metadata preview card.
- Live Windows DWM host backdrop replaces wallpaper-file rendering; the background source is the content behind the window.
- Fixes for Spotlight initial layout, row selection, empty results and resize rendering.
- Fixed restoring transparent child controls after transparency is toggled off and on.

## Validation and limitations
- 40 automated tests passed locally on Windows, including native controls, layout, search/cache correctness and experimental shader compilation.
- Live glass remains experimental: final screen verification, mixed-DPI and multi-monitor checks are pending. Shader compilation does not establish live nonlinear refraction.
- Stronger custom blur and the requested blue glass tint are deferred to a later build.
- Unsigned beta binaries. NTFS/USN access may require administrator rights; cached-index freshness is shown in the application.

Release checks: cargo test --workspace (40 passed), cargo clippy --workspace --all-targets -- -D warnings (passed), release build (passed), packaged hidden GUI diagnostic (exit 0, 44 geometry checks). The hidden diagnostic does not validate compositor output.
