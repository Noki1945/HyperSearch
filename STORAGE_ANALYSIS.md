# Storage analysis and privacy — 0.2

Settings → Storage offers disk/folder scope (empty means the current indexed folder), JSON/CSV/Markdown, depth, minimum file size, exclusions, safe/neutral names and compact/full reports. A configuration/privacy preview precedes the save dialog. Progress and cancellation remain available while searching continues. AI Report produces compact Markdown; Copy AI Request copies review instructions. Nothing is sent to an AI service.

The analyzer streams existing indexed names/parents and fresh symlink_metadata. It does not read file contents, hash, delete files or use the network. One worker processes metadata. Directory aggregates/ancestor checks scale with directory count, not all file metadata; top lists and buffers are bounded. Exclusive file creation prevents overwriting existing reports.

Object fields include name/path/parent/depth/type/extension, logical file size, available timestamps, attributes and reparse status. Full reports additionally stream directory aggregates: logical total, nested file/folder counts and share. CSV uses directory_summary/summary in the kind column and structured JSON in the warning column for aggregates. CSV text is quoted and formula-leading values escaped. JSON schema: hypersearch-storage-1.

The summary includes volume capacity/available/used-or-unavailable-to-caller bytes, counts, top 100 files/directories, extension/category distributions, old large files (100 MiB and 90 days), large temporary directories and review candidates. Volume counters describe the containing volume independently of the selected subtree/quotas. Nested directory totals overlap: never add them together. Size/age/extension do not prove an object is disposable. Protected/application paths advise Windows cleanup or application uninstallation.

Allocated/physical bytes are explicitly unknown. Logical size is counted per indexed pathname: hard links can be counted repeatedly, and USN can omit alternate hard-link names. Sparse/compressed files, inaccessible metadata and stale entries further prevent equating logical totals with occupied space. Reparse points and indexed descendants beneath reparse/inaccessible ancestors are not followed. Unavailable, excluded, reparse, omitted/aggregated, snapshot-warning and cancellation counters are explicit. Files can change during analysis. Confirmed duplicates are never claimed without hashing; this analyzer does not hash.

Safe mode masks the component after Users. Neutral names replace path/name components with deterministic identifiers; this is pseudonymization, not cryptographic anonymity. Exclusions run before metadata collection/aggregation. Depth/minimum-size limits omit detailed rows/top entries while retaining aggregate totals and explicit omission counts. Real filenames can remain sensitive after masking a username. Full mode deliberately preserves real names/paths. Review reports before sharing.

Full reports rotate after 50,000 object/directory rows into .part0002 and later files. Each JSON part is complete; CSV headers repeat; earlier parts name the next file. The final part contains whole-analysis totals and the part manifest. Compact reports retain methodology, warnings and the AI prompt. Cancellation closes the current part and marks a partial report. I/O failure can leave earlier valid parts and an incomplete final part; the UI reports failure.

CLI example:

```powershell
hypersearch-cli export --root C:\Data --output report.json --format json --depth 5 --min-bytes 1048576 --exclude 'C:\Data\Private;C:\Data\Work' --neutral-names --confirm
```

Omit --confirm for a configuration preview only. Also supported: --compact, --full-paths, --part-rows N (zero disables splitting). GUI cancellation produces a marked report; externally killing the CLI can leave an incomplete final file.

## WinDirStat

Startup and opening Settings discover remembered, registry/App Paths, standard installations, WinGet links and bounded Downloads/portable folders. Detected/manual executables persist and are revalidated; disappeared executables are re-discovered. Downloaded packages are remembered separately. Installed WinDirStat is preferred. Otherwise ZIP opens as an archive; an installer opens its containing folder without executing it. With no recognized package, the button starts the verified download described below; the site button opens [windirstat.net](https://windirstat.net/) and update checking opens the [official releases](https://github.com/windirstat/windirstat/releases/latest).

When no installed/external package is found, an explicit click starts an optional inbox Windows PowerShell helper, outside the UI/search worker. It fetches the official repository's latest x64 MSI asset metadata over HTTPS, requires its SHA-256 digest and pinned GitHub release-download origin, downloads to a partial file and checks SHA-256 before caching it. No downloaded script or execution-policy bypass is used. The package and verification record persist; repeating the action rehashes and reuses a matching cached package without downloading it again. Concurrent app instances serialize cache writes. The same button cancels an active download.

Only after verification does a separate Yes/No confirmation offer to open the standard installer. No leaves the package cached; closing HyperSearch cancels pending work and never offers installation. Windows handles its normal installer/UAC UI; no silent switches, protection changes or bypasses are used. A missing helper/hash, download/verification error or declined installation does not execute an installer, and the official-site button remains available. Update checking opens the official releases page. Unmanaged ZIP/MSI downloads remain archive/folder actions rather than automatically trusted installers.

The context-menu folder analysis passes one target through Windows process argument quoting, following the project's [command-line guide](https://github.com/windirstat/windirstat/wiki/Command-Line-and-CSV). The external program's behavior/version remains its own responsibility. Its portable 2.9.3 x64 build was separately launched against a two-file Unicode fixture and completed a documented CSV export with exit code zero, using an isolated portable INI. The verified archive/installer and private fixture CSV are not bundled in HyperSearch releases.

Actual download, cache reuse, active cancellation and tampered-cache recovery are recorded in benchmarks/0.2-windirstat-*.txt. The first test fetched official 2.9.3; a second reported a cache hit; deliberate corruption caused re-verification/redownload and restored the official digest. No installer was executed by these tests. Automated cases also reject malformed verification/path data and inaccessible cache roots, and verify that closing with a pending package does not open an installation confirmation.

Tests cover all formats, Unicode, username/neutral masking, exclusions, CSV formula text, compact mode, valid split parts/no lost rows, no overwrite, long paths, hard-link logical counting, missing metadata, pre-cancellation and mid-traversal cancellation. Tests do not execute installers, transmit exports or clean up user files.

## Measured full-snapshot analysis

The compact safe/neutral JSON test used the preserved 1,281,006-object C: snapshot and normal user permissions. Metadata traversal/export took 101,346.315 ms. It counted 1,105,052 file records and 169,367 directory records, reported 12,724 unavailable records and 166 reparse skips, and completed without cancellation. The report itself remains private and is not a release asset.

This standalone analyzer process did not build SearchIndex. Private RAM after snapshot load was 124,682,240 bytes (118.9 MiB), after export 143,360,000 (136.7 MiB); peak working set including snapshot loading was 268,857,344 bytes (256.4 MiB). Peak commit was 268,013,568 bytes. These are process/phase observations, not a precise live-allocation profiler. The test used a stale snapshot and changing/protected files, so it does not establish complete coverage or accurate physical disk usage. Public performance counters only are in benchmarks/0.2-storage.txt.

Windows/MSVC CI exposed an exclusion mismatch between canonical long names and temporary 8.3 paths. Exclusions now resolve aliases/dot segments and existing ancestors of missing indexed paths before metadata collection. Both the affected CI case and an absent-private-directory regression pass; excluded filenames are checked to be absent from output.

Indexing exclusions in index.indexing are inherited by GUI and CLI exports. Directory anchors retained for USN resolution never bypass report exclusions. Removing an indexing exclusion requires rebuilding to recover omitted file names. Source configuration errors stop export before creating a report. The WinDirStat helper also checks Authenticode on both new and cached MSI packages; invalid/unverifiable signatures fall back to official pages. Unsigned official packages still require a matching published SHA-256.
