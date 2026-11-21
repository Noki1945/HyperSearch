# UI 0.2 — development checkpoint

Not a completed 0.2 release. Work is local on `development/0.2`; no release publication is authorized.

Baseline before changes: 1,281,006 indexed objects; ready-made fast cache, engine readiness 1029.126 ms, first measured `excel.exe` query with path 0.138 ms. See `benchmarks/0.2-baseline.txt`. These are warm OS-cache library measurements, not complete GUI latency.

Implemented foundations: system/light/dark title-bar preference, rounded DWM window corners, system backdrop request and persistent transparency choice. Unsupported Windows attributes fall back harmlessly. Client controls are still standard Win32 controls: this is **not** a complete Liquid Glass renderer or complete dark client-area theme. No rendering timer, decorative GPU loop or animation loop was introduced. The saved animation preference is not yet connected to animations.

WinDirStat integration discovers remembered/registry/standard installation paths; validates that the executable still exists; re-discovers after disappearance; persists successful detection and manual choices. It only runs after an explicit click. Download/update buttons currently open official pages; verified automatic installer download/confirmation remains pending.

Latest local checks: 20 tests pass, Clippy with denied warnings passes, formatting and release build pass. The two new tests cover appearance preferences and WinDirStat remember/disappearance behavior. They do not run an installer or WinDirStat. No visual inspection, DPI/multi-monitor validation or measured complete UI latency is claimed. Screen capture remains off.

Pending: full composition/client-area renderer, separate settings window, Spotlight/hotkey consent, virtualized views/icons, filters/ranking/metadata preview, completed RAM page, background incremental search-table updates and final comparative benchmarks.
