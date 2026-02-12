# Oxide ⚡

**The High-Performance, Open-Source Disk Space Analyzer.**

Oxide is a lightning-fast, memory-efficient disk visualizer for Windows, built with the power of Rust and the safety of Data-Oriented Design.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE-MIT)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE-APACHE)

## 🚀 Key Features

- **Blazing Speed:** Direct NTFS MFT parsing bypasses slow system APIs. Target: 1M files/sec.
- **Ultra-Low Memory:** Handles 10M files in <500MB RAM using a flat-tree architecture.
- **GPU-Accelerated Visuals:** High-performance squarified treemaps rendered via Canvas/WebGL.
- **Open Source:** Transparent, community-driven development.

## 🛠️ Technical Stack

- **Backend:** Rust 🦀 (MFT parsing, local DB)
- **Frontend:** Tauri + Svelte + TypeScript
- **Rendering:** HTML5 Canvas / WebGL
- **Data Model:** Struct-of-Arrays (SoA) for cache-friendly processing.

## 📖 Documentation

- [Project Overview](docs/PROJECT_OVERVIEW.md)
- [Contributing Guide](CONTRIBUTING.md)
- [Security Policy](SECURITY.md)

## 🚦 Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install)
- [Node.js](https://nodejs.org/) & `pnpm`
- Visual Studio C++ Build Tools

### Installation

1. Clone the repository.
2. Install frontend dependencies:
   ```bash
   pnpm install
   ```
3. Run in development mode (requires Admin):
   ```bash
   pnpm tauri dev
   ```

## ⚖️ License

Oxide is dual-licensed under the **MIT License** and the **Apache License 2.0**. You may choose either license at your option.
