# Oxide Project Overview

Oxide is a Windows disk space analyzer focused on one problem: scanning very large NTFS volumes quickly while using less memory than traditional desktop analyzers.

## Goals

- keep scan latency low on multi-million-node NTFS volumes
- keep post-scan RAM use small enough to stay practical on everyday machines
- make the resulting tree easy to explore through a treemap, folder navigator, and largest-files inspector
- keep the codebase measurable, so performance work is guided by benchmarks and phase timings instead of guesses

## Current Architecture

### Backend

- Rust/Tauri command layer in `src-tauri/src/lib.rs`
- NTFS scanning in `src-tauri/src/scan/mft.rs`
- filesystem fallback scanning in `src-tauri/src/scan/filesystem.rs`
- packed file-tree storage in `src-tauri/src/core/file_tree.rs`
- raw MFT parsing in `src-tauri/src/mft/parser.rs`

The backend owns the scan flow, in-memory file tree, treemap layout generation, and paged queries used by the UI.

### Frontend

- Svelte app rooted at `src/routes/+page.svelte`
- treemap canvas in `src/lib/components/Treemap.svelte`
- lazy tree navigator in `src/lib/components/TreeView.svelte`
- paged largest-files view in `src/lib/components/FileList.svelte`

The frontend is intentionally thin. It renders the active scan state, requests paged slices of data over Tauri IPC, and avoids building a DOM proportional to total file count.

## Performance Approach

Oxide currently gets most of its speed from:

- reading NTFS metadata directly instead of walking the filesystem API for the common case
- using compact Rust storage for nodes and names
- paging expensive views instead of materializing everything in the UI
- timing scan phases directly so regressions are visible

Current benchmark baselines are maintained in [BENCHMARKS.md](BENCHMARKS.md).

## Current Status

Implemented today:

- NTFS drive detection
- MFT scan path
- filesystem fallback path
- scan progress and phase timing
- treemap visualization
- lazy folder navigation
- paged largest-files inspection

Not implemented yet:

- file operations
- duplicate detection
- live refresh from the USN journal
- multi-drive comparisons
- export flows

## Roadmap

Short term:

- continue reducing MFT scan time
- keep UI responsiveness high without pushing work back into the scan loop
- harden public repository docs and contributor workflow

Medium term:

- improve non-NTFS fallback behavior
- add file operations and richer inspection workflows
- add export and reporting features
