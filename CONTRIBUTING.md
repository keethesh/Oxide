# Contributing to Oxide ⚡

First off, thanks for taking the time to contribute! It's people like you that make Oxide better.

## How Can I Contribute?

### Reporting Bugs
- Use the **Bug Report** template when opening an issue.
- Describe the expected behavior and what actually happened.
- Provide steps to reproduce the issue.

### Suggesting Enhancements
- Check the [Project Roadmap](docs/PROJECT_OVERVIEW.md#6-roadmap) to see if it's already planned.
- Use the **Feature Request** template.

### Pull Requests
1. Fork the repo and create your branch from `main`.
2. If you've added code that should be tested, add tests.
3. Ensure the test suite passes.
4. Update documentation if necessary.
5. Create a Pull Request with a clear description of the transition.

## Development Setup

### Rust Backend
```bash
cargo build
cargo test
```

### Frontend
```bash
pnpm install
pnpm dev
```

## Coding Standards
- **Rust:** Run `cargo fmt` and `cargo clippy` before committing.
- **JS/TS:** Use Prettier for formatting.
- **Commits:** Use [Conventional Commits](https://www.conventionalcommits.org/).
