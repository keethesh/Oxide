# Oxide

Oxide is a Windows-first NTFS disk space analyzer built with Rust, Tauri, Svelte, and TypeScript. It scans a drive by reading NTFS metadata directly from the Master File Table, then presents the result as a treemap, a lazy folder tree, and a largest-files view.

The current project goal is straightforward: beat traditional Windows disk analyzers on memory efficiency while staying fast enough to feel immediate on large drives.

## Current Baseline

Latest captured baseline on the maintainer's `C:` volume:

- `mode=Mft`
- `scan_ms=15081`
- `aggregate_ms=210`
- `largest_files_ms=175`
- `store_ms=13`
- `total_ms=15480`
- `358 MB` RAM after scan settled

That run covered `4,041,909` files and `733,450` folders, or `4,775,359` total nodes. Full comparison notes live in [docs/BENCHMARKS.md](docs/BENCHMARKS.md).

## MVP Scope

This repository currently implements an NTFS-only MVP:

- local drive discovery with NTFS support detection
- one-drive scan flow with progress reporting
- treemap navigation
- lazy folder hierarchy browsing
- paged largest-files list for the selected subtree

Out of scope for this MVP:

- deletion and recycle-bin actions
- live updates via the USN journal
- duplicate-file detection
- multi-drive comparison

## Requirements

- Windows
- Rust stable toolchain
- Node.js
- PNPM
- Visual Studio C++ Build Tools
- Administrator access is recommended to read the NTFS MFT at full speed

## What Exists Today

- raw NTFS MFT scanning with a filesystem fallback path
- packed Rust file-tree storage tuned for multi-million-node scans
- paged IPC queries for tree nodes and largest files
- treemap layout caching and canvas rendering
- benchmark and profiling hooks to track scan, aggregation, and indexing costs

## Local Development

```bash
pnpm install
pnpm check
pnpm build
cd src-tauri
cargo check
cd ..
pnpm tauri dev
```

Run the terminal as Administrator if you want the fast MFT scan path on local NTFS volumes.

## Current Verification

The intended verification flow for this MVP is:

- `pnpm check`
- `pnpm build`
- `cargo check`
- `cargo test`

If `cargo test` fails because the machine is low on free disk space, clear space on the system drive and rerun it before shipping.

## Benchmarks

Performance baselines and capture rules live in [docs/BENCHMARKS.md](docs/BENCHMARKS.md). Record scan duration, post-scan memory, scan mode, elevation state, and file/folder counts before comparing against other disk analyzers.

## Documentation

- [docs/PROJECT_OVERVIEW.md](docs/PROJECT_OVERVIEW.md) explains the current architecture and roadmap.
- [CONTRIBUTING.md](CONTRIBUTING.md) covers the contribution workflow.
- [SECURITY.md](SECURITY.md) explains how to report vulnerabilities.

## CI/CD

GitHub Actions is configured for two repository workflows:

- `CI` runs on pull requests and pushes to `main` on `windows-latest`, then executes `pnpm check`, `pnpm build`, and `cargo test`.
- `Release Main` runs on pushes to `main`, reads the repo SemVer version from `package.json`, and publishes a Windows Tauri release under that version tag.

Oxide uses SemVer with `0.1.0` as the current release line. When you intentionally bump versions, update `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json` together.

## License

Oxide is licensed under the MIT License. See [LICENSE](LICENSE).
