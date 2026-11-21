# Changelog

## 0.1.0-beta.1 — 2026-11-22

* Native Explorer-style Windows interface: drives sidebar, address bar, back/up navigation, direct directory children, search within current directory, details columns and opening files.
* Independent Rust search library and CLI, NTFS enumeration and USN checkpoints, safe directory fallback, Unicode filename search and pagination.
* Persistent, versioned and checked search-table caches with automatic rebuilding after corruption or incompatible source changes.
* Fast, balanced and economy memory profiles; first-run memory estimate and optional target budget. Fast is the testing default.
* Windows automated checks, live privileged NTFS validation, real and synthetic performance logs, reproducible release packaging.

This beta is a filename search application with basic read-only navigation. Continuous GUI monitoring, complete hard-link enumeration through USN, file operations and a fully validated multi-DPI interface remain future work.
