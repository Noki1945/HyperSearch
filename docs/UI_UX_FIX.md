# UI/UX correction validation - 2026-11-22

The screenshot supplied by the user demonstrates the original visual failure. This correction does not claim screenshot-based approval: screen capture remains disabled.

## Architecture decision

Use a hybrid native window: bounded owner-data lists, native edits/combos/keyboard/accessibility semantics, custom button/header surfaces and a single token palette. DWM decorates the non-client frame; GDI text always has an opaque client surface. Full-client negative glass margins were removed. Theme messages are handled before forwarding owned-window notifications, so settings and Spotlight receive the same colors. System high contrast overrides application palettes and disables backdrops. System context menus retain Windows rendering.

| Option | RAM/startup | Idle CPU/GPU | DPI | Accessibility | Maintenance |
|---|---|---|---|---|---|
| Chosen Win32 hybrid | No additional runtime; measured below | Event-driven drawing; no frame timer | Shared DIP geometry and native per-monitor awareness | Native control semantics retained; screen-reader usability unverified | Small custom surfaces, explicit color/geometry tests |
| Direct2D | Additional rendering resources; not benchmarked here | Accelerated drawing, must schedule invalidation | Explicit scaling required | Custom content needs accessibility providers | More rendering/device-loss code |
| DirectComposition | Composition resources; not benchmarked here | Retained GPU composition, suitable for limited transitions | Explicit visual transforms | Does not itself provide control semantics | Additional device/visual lifecycle |
| WinUI 3 | Framework dependency or bundled runtime; not benchmarked here | Framework-managed | Framework support | Rich standard controls | Rust interop/deployment migration and replacement of existing controls |

These are engineering tradeoffs, not measured cross-framework rankings. References: [DWM blur/alpha behavior](https://learn.microsoft.com/en-us/windows/win32/dwm/blur-ovw), [Direct2D](https://learn.microsoft.com/en-us/windows/win32/direct2d/direct2d-overview), [DirectComposition](https://learn.microsoft.com/en-us/windows/win32/directcomp/directcomposition-portal), [Windows App SDK deployment](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/deploy-overview), [Windows high contrast](https://learn.microsoft.com/en-us/windows/win32/winauto/high-contrast-parameter).

## Implemented changes

- One navigation/address/search/filter/settings row; filters expand on demand. Native tooltips describe compact buttons.
- Shared light/dark tokens for all text surfaces, borders, selection and status severity. Contrast tests cover primary/secondary text on surfaces and selection; disabled/warning/error text also checked against the base surface.
- Opaque color-message handling, event-driven rounded button surfaces and explicit list/header colors. No transparent static text over a full glass client area.
- Responsive sidebar and optional metadata pane, hidden automatically on narrow windows. Settings support scrolling when their content exceeds the available work area.
- Single-line status button; bounded detailed diagnostics are separate. USN permission failure leaves saved-index search available and offers settings/instructions without automatic UAC.
- Explicit initial/loading/empty/no-match/error text. Virtual Unicode callbacks remain in use.
- Page sorting by name/type/location/size/date/relevance. Size/date load at most 500 records asynchronously; unknown values remain at the end. Global sorting is deliberately not advertised: millions of matches are not materialized for it.
- Bounded persistent metadata cache (2,048 paths). Every reuse validates live size/write time/creation time/attributes; cached values do not override changed/deleted files. This reduces repeated formatting, not filesystem validation latency.
- Storage analysis publishes an immediate zero-metadata inventory/progress update and partial processed sizes/errors/largest file. Partial values are labelled incomplete. Optional current-folder priority uses streaming passes without a million-entry sort. Existing folder scope/cancellation/privacy remain intact. Full-disk scan speed is not claimed improved from the earlier 101-second measurement.

## Verification

39 automated tests: the original 32 plus responsive geometry/preview, WCAG contrast, real DC color switching/opaque backgrounds, compact bounded diagnostics, persistent metadata invalidation and prioritized streaming/progress regression cases. Formatting, Clippy with denied warnings and Windows x64 release build pass locally.

Pure geometry checks cover 1280x720, 1920x1080 and 2560x1440 at 96/120/144/192 DPI, plus window reduction. The real application diagnostic checks 44 native control boundaries over four window sizes at this machine's actual 96 DPI. Native list Unicode/selection tests remain in the suite. High-contrast colors are tested with an injected palette and the runtime reads SPI_GETHIGHCONTRAST; the user's OS setting was not changed.

A visible diagnostic was actually run without screen capture. It records native redraw/scroll/Spotlight dispatch and custom-draw counters. These are synchronous CPU/GDI measurements, not compositor presentation or keystroke-to-photon timing. The scroll sample uses the four matching excel.exe rows, so it is a dispatch smoke measurement rather than a large-list scrolling benchmark. Actual mixed-monitor DPI, perceptual hover quality, screen-reader usability and GPU traces remain unverified.

Before/after and visible logs: benchmarks/0.2-ux-before.txt, 0.2-ux-after.txt, 0.2-ux-paint.txt. The before build lacked paint instrumentation, so no fabricated before/after paint comparison is supplied. Hidden startup was about 1.40 seconds both before and after; observed private RAM remained about 459.6 MiB. Individual request timings vary and are not a statistically controlled speedup estimate. The visible run recorded redraw dispatch 8.623 ms, 20 scroll dispatches 2.884 ms and Spotlight opening 10.704 ms; ten seconds of visible idle added zero process CPU seconds at counter resolution.

## Manual acceptance checklist

Open dist/0.2-ux/HyperSearch.exe. Check Light/Dark/System themes, filters, tooltips, empty queries, a missing folder, selection, resize, preview toggle, all settings sections and scrollbars, then Spotlight and metadata sorting. At each display scale, check readability, clipping and overlap. Check Windows high contrast manually if desired; HyperSearch does not change that setting. Compare the result with the supplied screenshot. No screenshots were captured by this work.

## Privilege separation assessment

USN/MFT access is the specific privileged operation. A future read-only helper could enumerate a user-selected volume and return validated bounded records to the unelevated GUI. It needs a versioned protocol, restrictive named-pipe ACLs, server/client identity checks, strict root authorization and cancellation. This correction retains explicit Windows Run-as as an option and never silently elevates the GUI or starts a privileged service. No helper is represented as implemented.

No new GitHub release is published by this correction. The previously published beta remains unchanged.

## Screen-verified reference layout update - 2026-11-22

The user subsequently authorized screen capture. Actual screenshots were inspected through Computer Use in this chat: light/system and dark appearance, search results, selected-file metadata, settings appearance/storage, expanded filters, a reduced window, empty results and Spotlight. This supersedes the earlier screen-capture restriction for this verification.

Screen inspection found and corrected a blank Spotlight: its children were created after the initial WM_SIZE and retained zero bounds if the outer size did not change. Opening now explicitly lays out its controls; the native diagnostic rejects zero child bounds. Additional fixes: no-match text below the header, selection colors based on actual LVIS_SELECTED state, complete resize invalidation, shorter toolbar labels and removal of unnecessary settings scrollbars.

The user then supplied a blue glass-inspired reference. The implemented layout uses a blue palette/gradient, rounded input/surface framing, compact category buttons wired to existing filters, a narrower sidebar, name/type/size/date table and a selected-item card with a system type icon plus open/location/copy actions. Full paths remain in the card and Spotlight. Metadata for bounded pages (at most 100 per result event) is loaded on the metadata worker; unknown directory sizes are not fabricated. No search engine rewrite or unbounded global-sort materialization was added.

The final build was opened and inspected on the real Program Files folder with the Common Files row selected. Screenshots in this chat show the running application, not a generated mockup. There is no optical glass refraction shader; native-control details still differ from the reference. Physical mixed-DPI/multi-monitor testing remains unverified. No release was published.

Local verification: 39 workspace tests, formatting, Clippy with warnings denied and Windows x64 release build passed. Current executable: dist/0.2-ux/HyperSearch.exe.
