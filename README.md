# Oxide

**A Windows disk observatory for people who want to know where the space went without donating half their RAM to the search.**

Oxide is a Windows-first disk space analyzer built with Rust, Tauri, Svelte, and TypeScript. It reads NTFS metadata directly from the Master File Table when it can, falls back to filesystem walking when it must, and presents the result as a fast, map-first interface for exploring very large drives.

The project has a simple bias: scan the drive, keep memory low, make the heavy parts measurable, and let the UI stay out of the way.

## Why Oxide Exists

Traditional disk analyzers are useful, but they can feel expensive on modern Windows machines with millions of files. Oxide is an attempt to keep the good part, the immediate sense of what is consuming space, while being more careful with memory and data flow.

Current priorities:

- **Fast NTFS scans** through direct MFT reads on supported volumes.
- **Low post-scan memory use** through compact Rust-side storage.
- **Progressive exploration** with a treemap, lazy folder tree, and paged largest-file lists.
- **Honest performance work** with phase timings and reproducible benchmark notes.

## What It Does Today

Oxide currently implements an NTFS-focused MVP:

- discovers local drives and detects NTFS support
- scans one drive at a time with progress reporting
- uses a fast MFT path when permissions allow it
- falls back to normal filesystem traversal when raw NTFS access is unavailable
- renders a navigable treemap on canvas
- browses folders lazily instead of building a giant DOM
- pages largest-file results for the selected subtree
- records scan, aggregation, indexing, and storage timings

Not implemented yet:

- deletion or recycle-bin actions
- live updates from the USN journal
- duplicate-file detection
- multi-drive comparison
- polished export/report workflows

## Performance Snapshot

Latest captured local baseline on the maintainer's `C:` volume:

| Metric | Oxide 0.2.2 | **Oxide 0.3.0** |
| --- | ---: | ---: |
| Scan mode | `mft` | `mft` |
| Files | `4,041,909` | `4,841,051` |
| Folders | `733,450` | _\~same_ |
| Total nodes | `4,775,359` | `4,841,052` |
| Data size | `369 GB` | _\~same_ |
| Scan phase | `15.08 s` | **`9.05 s` (−40%)** |
| Aggregation | `210 ms` | `288 ms` (release) |
| Largest-file index | `175 ms` | **`33 ms` (−81%)** |
| Total backend time | `15.48 s` | **`9.4 s` (−39%)** |
| Memory after scan | `358 MB` | _\~same_ |

Key improvements in 0.3.0:
- **Release LTO build** with `opt-level=3`, `lto="fat"`, `codegen-units=1`
- **CLI profile mode** (`oxide.exe profile <drive>`) for automated benchmarking
- **Debounced search inputs** (150ms) in folder/file views
- **`VecDeque` LRU caches** for O(1) eviction (was O(n))
- **MFT fixup optimization** — skip copy when not needed
- **Partial sort** for treemap overflow inputs (`select_nth_unstable`)

These are engineering baselines, not universal claims. Disk cache state, elevation, drive type, antivirus activity, and file churn all matter. See [docs/BENCHMARKS.md](docs/BENCHMARKS.md) for capture rules and comparison notes.

## Interface

Oxide is built around a compact workspace:

- **Treemap** for spatially reading the drive at a glance.
- **Folder navigator** for drilling through hierarchy without loading every node into the UI.
- **Largest files** for finding heavy individual files inside the selected scope.
- **Telemetry bar** for scan mode, timing, and phase information.

The frontend stays intentionally thin. The Rust backend owns the scan, in-memory tree, layout generation, and paged queries. The UI asks for slices of data and renders them.

## Requirements

- Windows
- Rust stable toolchain
- Node.js
- PNPM
- Visual Studio C++ Build Tools
- Administrator access for the fastest MFT scan path

Oxide can run without elevation, but raw NTFS reads may fail and trigger the slower filesystem fallback.

## Local Development

Install dependencies:

```bash
pnpm install
```

Run frontend checks:

```bash
pnpm check
pnpm build
```

Run Rust checks:

```bash
cd src-tauri
cargo check
cargo test
cd ..
```

Start the desktop app:

```bash
pnpm tauri dev
```

For realistic NTFS scan testing, run the terminal as Administrator before starting the app.

## Repository Map

| Path | Purpose |
| --- | --- |
| `src/routes/+page.svelte` | Main Svelte app shell and scan flow |
| `src/lib/components/Treemap.svelte` | Canvas treemap renderer |
| `src/lib/components/TreeView.svelte` | Lazy folder hierarchy browser |
| `src/lib/components/FileList.svelte` | Paged largest-file view |
| `src-tauri/src/lib.rs` | Tauri command layer and scan orchestration |
| `src-tauri/src/scan/mft.rs` | NTFS MFT scan path |
| `src-tauri/src/scan/filesystem.rs` | Filesystem fallback scan path |
| `src-tauri/src/core/file_tree.rs` | Packed file-tree storage |
| `docs/BENCHMARKS.md` | Benchmark capture rules and results |
| `docs/PROJECT_OVERVIEW.md` | Architecture notes and roadmap |

## Verification

Before shipping a meaningful change, run the checks that match the blast radius:

```bash
pnpm check
pnpm build
cd src-tauri
cargo test
```

For performance changes, include before/after numbers and describe the scan environment. Warm cache versus cold cache is not a footnote; it can change the story.

## Contributing

The best contributions are focused and measurable: bug fixes, reproducible performance reports, UI clarity improvements, and docs that match the current behavior.

Start with [CONTRIBUTING.md](CONTRIBUTING.md). For security reports, use [SECURITY.md](SECURITY.md).

## License

Oxide is licensed under the MIT License. See [LICENSE](LICENSE).
