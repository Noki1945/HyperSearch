# Beta 0.1 — test report

Date: 2026-11-22. Local platform: Windows x64, AMD Ryzen 7 9800X3D, 32 GB RAM, Rust 1.100.0 GNU. All numbers are machine-specific observations, not guaranteed service levels.

## Automated coverage

18 tests: 2 USN binary parser cases; 10 engine integration cases; 3 persistent-cache cases; 3 GUI/model/preferences cases. Coverage includes:

* Unicode exact, prefix, substring, multiple tokens, extension/path filters, page cursors and zero limits.
* Equal results across all three memory modes, duplicate normalized names and cancellation.
* Direct-child directory browsing, parent moves, missing directories and graph corruption.
* Snapshot round trips, checksum corruption and atomic replacement.
* Cache hits, Unicode paths, identical pagination, stale names, mode mismatch, corrupted checksum, checkpoint-only changes, empty indexes and an unwritable cache directory.
* Application-state exclusion to prevent self-invalidating caches.
* Memory estimation and custom-budget rejection, saved settings and invalid settings.
* An isolated, invisible native ListView: three columns, Unicode result text, selection and a folder-navigation request.

Debug tests, release tests, formatting, Clippy with denied warnings and release builds are required before tagging. CI repeats the formatting, Clippy, tests and build on Windows/MSVC. Privileged NTFS tests are separate and are not silently represented as ordinary CI coverage.

## Live filesystem checks

The elevated C: test passed NTFS enumeration (about 1.28 million records in 2.95 s), create/delete/rename/move, parent-directory rename, checkpoint reload and rejection of a deliberately mismatched journal ID/expired checkpoint. It uses its own temporary files and does not recreate or clear the journal. Details and exclusions are in `VALIDATION.md` and `benchmarks/ntfs-admin.txt`.

The `excel.exe` global query found the existing Office executable. First 100 results are materialized; the full index is loaded before timing a warm query. A separate 20,000-file fixture matched Explorer's result counts for three filename queries. Explorer UI observations include automation overhead, so there is no defensible cross-product speedup factor.

## RAM and cached startup

On the real 1,281,006-object snapshot, observed private process memory was 237/392/503 MB for economy/balanced/fast. Economy p99 was at most about 43 ms in the nine-query corpus, below the 200 ms engineering target. These timings exclude initial loading and GUI drawing. See `MEMORY_MODES.md` and the three `benchmarks/memory-*.txt` profile logs.

Fast persisted tables occupy 301,817,136 bytes. Initial prepare-and-save: 3482.551 ms. Separate-process cached prepare: 1057.919 ms. Final beta repeat: 991.433 ms, cache hit confirmed. The OS file cache was warm; true cold start was not measured. Source logs: `benchmarks/cache-first.txt`, `benchmarks/cache-repeat.txt`, `benchmarks/beta-cache-repeat.txt`.

A hidden final-GUI startup remained alive after 4 seconds, with about 510 MB private memory and 477 MB working set, using persisted fast preferences. See `benchmarks/beta-gui-startup.json`. No screen capture was used. This establishes a startup smoke check, not visual correctness or a precise UI-ready latency.

## Known gaps

Actual journal wrap/recreation, disconnected volumes, power-loss recovery, long-running GUI monitoring, all hard-link names in USN mode, sorting columns, accessibility usability, arbitrary DPI/multiple monitors and a visual review of the complete window remain unverified or unimplemented. No claim of superiority to Everything is made. The beta assets are unsigned. Personal index/cache/settings files are excluded from publication; log home-directory names are redacted where applicable.
