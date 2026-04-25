# Contributing to Oxide

Oxide is still evolving quickly, so the most useful contributions are narrowly scoped fixes, reproducible performance reports, and documentation updates that reflect the current code instead of the intended future state.

## Before Opening a Pull Request

- Check whether the change already exists on `main`.
- Keep the change focused. Separate refactors from behavior changes.
- Add or update tests when the change affects shared behavior.
- Update docs if the public behavior, workflow, or benchmark story changed.

## Development Setup

```bash
pnpm install
pnpm check
pnpm build
cd src-tauri
cargo test
cd ..
pnpm tauri dev
```

Run the terminal as Administrator if you want to exercise the fast NTFS MFT scan path.

## Verification Expectations

Before submitting a change, run the checks that match its risk:

- `pnpm check`
- `pnpm build`
- `cargo test`

If a change is explicitly performance-related, include the before/after numbers and describe how the run was captured.

## Issue Reports

Useful bug reports include:

- Windows version
- whether the app was elevated
- scanned filesystem type
- exact steps to reproduce
- observed behavior
- expected behavior
- logs or screenshots when relevant

## Performance Work

Performance changes should be grounded in measurements. If you change scan speed, RAM use, query latency, or rendering behavior, update [docs/BENCHMARKS.md](docs/BENCHMARKS.md) or explain why the existing baseline should remain unchanged.
