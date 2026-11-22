# Changelog

## 0.3.0-beta.1 ? 2026-11-22

* Blue Explorer-inspired layout, compact native file icons and metadata preview card.
* Live DWM host backdrop instead of reading a wallpaper image.
* Fixed Spotlight initial layout, selection drawing, empty states and resize rendering.
* Experimental glass: stronger custom blur, blue glass tint and nonlinear live refraction are not included yet. Final live-backdrop screen verification and mixed-DPI testing remain pending.


## 0.2.0-beta.1 — 2026-11-22

* Native DWM glass-inspired surfaces, light/dark/system appearance, optional transparency/animation, scalable Segoe UI controls and separate seven-section settings.
* Separate Spotlight with opt-in configurable global hotkey, conflict handling, shared search index and cancellation.
* Virtual details/list/icon views, multiple selection, sorting of loaded pages, context actions, forward navigation, redirected Known Folder favorites, pins and recent/opening-frequency history.
* Category/extension/path filters, optional bounded relevance ranking, asynchronous metadata and bounded native type icons.
* Persistent FRN lookups and incremental search postings; real elevated NTFS create/rename/move/parent-rename/delete validation without post-initial full rebuilds.
* Cancellable local JSON/CSV/Markdown storage reports, privacy/exclusions, directory/category/top summaries, review cautions and automatically split full reports.
* Persistent discovery of installed/portable WinDirStat and downloaded packages; cancellable official SHA-256/Authenticode-verified x64 downloads, cache reuse and explicit installer consent. Official pages remain a fallback after verification/download failure.
* Saved indexing exclusions, USN freshness/update status, file/folder estimates, mode explanations, text sizing, optional information pane and Spotlight type/location controls.
* Reduced snapshot/cache over-allocation, measured memory modes, hidden GUI pipeline diagnostic and expanded regression tests. Physical mixed-DPI/multi-monitor visual validation remains a beta limitation; screen capture stays disabled.

## 0.1.0-beta.1 — 2026-11-22

* Native Explorer-style Windows interface: drives sidebar, address bar, back/up navigation, direct directory children, search within current directory, details columns and opening files.
* Independent Rust search library and CLI, NTFS enumeration and USN checkpoints, safe directory fallback, Unicode filename search and pagination.
* Persistent, versioned and checked search-table caches with automatic rebuilding after corruption or incompatible source changes.
* Fast, balanced and economy memory profiles; first-run memory estimate and optional target budget. Fast is the testing default.
* Windows automated checks, live privileged NTFS validation, real and synthetic performance logs, reproducible release packaging.

This beta is a filename search application with basic read-only navigation. Continuous GUI monitoring, complete hard-link enumeration through USN, file operations and a fully validated multi-DPI interface remain future work.
