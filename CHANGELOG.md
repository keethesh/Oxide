# Changelog

All notable changes to Oxide will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- CLI profile mode for automated benchmarking (`oxide.exe profile <drive>`)

### Changed
- Performance improvements across backend and frontend
- Release build now uses LTO (Link Time Optimization)
- Debounced search inputs in TreeView and FileList (150ms)
- `VecDeque` for LRU cache eviction (O(1) vs O(n))
- Skip MFT fixup copy when not needed
- Partial sort for treemap overflow inputs

## [0.3.0] - 2026-04-27

### Added
- CLI profile mode (`oxide.exe profile <drive>`) for automated benchmarking
- `ProgressSink` trait to abstract over Tauri window vs CLI progress reporting
- `WindowProgressSink` for GUI progress emission
- `SilentProgressSink` for headless profiling

### Changed
- **Performance**: ~40% faster MFT scans in release builds (15.08s → 9.05s)
- **Performance**: ~81% faster index times (175ms → 33ms in release)
- **Performance**: ~39% faster total backend time (15.48s → 9.4s)
- Release profile with LTO fat, opt-level=3, codegen-units=1, strip=true
- `VecDeque` for O(1) LRU cache eviction (was O(n) with `Vec`)
- Debounced search inputs (150ms) in TreeView and FileList components
- `Cow<'_, str>` for `node_name_ref` to reduce allocations
- Pre-allocated capacity for `get_full_path` and `get_file_path`
- Skip MFT fixup copy when single-sector records don't need it
- Partial sort for treemap inputs using `select_nth_unstable`
- Skip `scale_layout` clone when cached dimensions match exactly

### Fixed
- Borrow checker issue in `TreemapLayoutCache::get`

## [0.2.2] - 2026-04-27

### Added
- Subphase scan profiling (parse_ms, ingest_ms, link_ms)

### Changed
- Preallocate MFT links vector based on total records
- Throttle filesystem progress updates

### Fixed
- Child paging cache and MFT linking improvements

## [0.2.1] - 2026-04-26

### Added
- MFT-based NTFS scan implementation
- Filesystem fallback scan path
- Treemap visualization with canvas rendering
- Lazy folder tree with pagination
- Largest files view with pagination
- Drive detection and NTFS support checking
- Administrator elevation for raw MFT access
- Scan progress reporting with phase information
- Export scan snapshot to clipboard

### Changed
- Improved backend query performance
- Optimized MFT record linking
- Enhanced UI with new logo-led workspace

### Fixed
- MFT parser fixes for edge cases
- Child paging cache behavior
- File path reconstruction

## [0.2.0] - 2026-04-25

### Added
- Initial release with core functionality
- SvelteKit + Tauri desktop app shell
- Basic disk space analysis workflow
- Scan mode selection (MFT vs filesystem)
- Tree view and file list components

[unreleased]: https://github.com/keethesh/oxide/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/keethesh/oxide/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/keethesh/oxide/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/keethesh/oxide/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/keethesh/oxide/releases/tag/v0.2.0
