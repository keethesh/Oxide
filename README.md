# Oxide

Oxide is a Windows-first NTFS disk space analyzer built with Rust, Tauri, Svelte, and TypeScript. It scans a drive by reading NTFS metadata, then presents the results as a treemap, a lazy folder tree, and a largest-files view.

## MVP Scope

This repository currently implements an NTFS-only MVP:

- local drive discovery with NTFS support detection
- one-drive scan flow with progress reporting
- treemap navigation
- lazy folder hierarchy browsing
- paged largest-files list for the selected subtree

Out of scope for this MVP:

- non-NTFS fallback scanning
- non-admin fallback handling
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
- Administrator access may be required to read the NTFS MFT

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

## Current Verification

The intended verification flow for this MVP is:

- `pnpm check`
- `pnpm build`
- `cargo check`
- `cargo test`

If `cargo test` fails because the machine is low on free disk space, clear space on the system drive and rerun it before shipping.
