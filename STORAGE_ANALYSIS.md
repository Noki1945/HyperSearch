# Storage analysis — 0.2 implementation status

The detailed requirements are recorded in `docs/0.2-requirements.txt`. Storage scanning and JSON/CSV/Markdown export are **not implemented at this checkpoint**. No user disk has been scanned for sizes, no personal export has been created and no data has been transmitted to an AI service.

Required implementation contract: use indexed structure, collect filesystem metadata separately and only on request, stream full exports, bound top lists and buffers, support cancellation and exclusions, aggregate directory logical totals with explicit incomplete/error counts. Allocated/physical bytes must remain unknown unless a suitable Windows API supplies a defensible value. Hard-link identity, sparse/compressed data and non-followed reparse points require explicit accounting notes. Never infer confirmed duplicates from names/sizes alone.

Privacy contract: preview before export, safe mode with username masking and optional neutral sensitive-folder names, depth/size/compact controls with explicit skipped counts, no network transmission and no automatic cleanup. System paths must not be labeled safe to delete. Installer integration must never disable Windows protection or run an unverified download automatically.
