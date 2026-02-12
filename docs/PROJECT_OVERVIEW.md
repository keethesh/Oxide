# Oxide ⚡
**The High-Performance, Open-Source Disk Space Analyzer.**

> **Mission:** To build the fastest, most memory-efficient disk visualizer for Windows, utilizing the raw power of the NTFS Master File Table (MFT) and the safety of Rust.

### 1. The Problem
Existing disk analyzers (like WizTree or WinDirStat) are excellent tools, but they often suffer from:
*   **Closed Source:** No community contributions or transparency.
*   **High Memory Usage:** Scanning a large drive (10M+ files) can consume 2GB–4GB of RAM due to object overhead.
*   **UI Lag:** Rendering millions of file nodes often freezes the interface.

### 2. The Oxide Solution
Oxide is designed from the ground up to handle **10 million files in under 500MB of RAM**.

We achieve this by abandoning standard Object-Oriented programming in favor of **Data-Oriented Design**. We do not create "File Objects." We create flat, packed arrays of data.

### 3. Technical Architecture

#### The Stack
*   **Core / Backend:** **Rust** 🦀
    *   Handles direct NTFS MFT parsing (unsafe WinAPI calls wrapped in safe abstractions).
    *   Manages the in-memory file database.
*   **Frontend:** **Tauri** (WebView2) + **Svelte** + **TypeScript**
    *   Provides a native-feeling, lightweight UI.
    *   Communicates with Rust via the Tauri IPC bridge.
*   **Rendering:** **HTML5 Canvas / WebGL**
    *   The Treemap is rendered as a single GPU-accelerated image, not thousands of DOM elements.

#### The "Flat Tree" Data Structure
To minimize RAM, we avoid pointers and classes. The entire file system is represented by a "Struct of Arrays" or a flat vector of packed structs.

**The File Entry (Rust):**
```rust
#[repr(C)] // Natural alignment, no UB risk
struct FileEntry {
    size: u64,              // File size in bytes (8 bytes, aligned to 8)
    parent_index: u32,      // Index of parent in the vector
    first_child_index: u32, // Index of first child (Linked List approach)
    next_sibling_index: u32,// Index of next sibling
    name_offset: u32,       // Offset into the global String Arena
    name_len: u16,          // Length of filename
    flags: u16,             // Attributes (IsDirectory, ReadOnly, etc.)
}
```
*   **Memory Cost:** 28 bytes per file (naturally aligned, no padding needed).
*   **Total RAM for 10M files:** ~280MB (plus string storage).
*   **Why `#[repr(C)]` instead of `#[repr(packed)]`?** Packed structs create unaligned memory accesses (slower on x86_64 and undefined behavior when taking field references). By ordering fields largest-to-smallest, we achieve 28 bytes with zero padding and full memory safety.

#### String Deduplication (The Arena)
We do not store full paths (e.g., `C:\Windows\System32\notepad.exe`).
1.  We store all filenames in a single massive `Vec<u8>` (The Arena).
2.  `FileEntry` points to an offset in this arena.
3.  Full paths are reconstructed on-the-fly by traversing `parent_index` up to the root.

### 4. Core Features (MVP)

#### 1. MFT Parsing (The Speed Engine)
*   Bypass Windows `FindFirstFile` API.
*   Read the raw `$MFT` directly from the disk volume.
*   **Target Speed:** Scan 1 million files per second.

#### 2. The Treemap (Visualizer)
*   A **Squarified Treemap** algorithm to visualize space usage.
*   **Layout Computation:** Calculated in Rust and sent as a flat `Vec<Rect>` buffer over IPC for maximum performance.
*   **Rendering:** Canvas API (potentially WebGL) for 60FPS zooming and panning.
*   **Visual Enhancements:**
    *   Cushion shading for depth perception (à la WinDirStat).
    *   Color-coded by file type category (media, docs, code, system).
*   **Interactive hover states** calculated via coordinate mapping, not DOM events.
*   **Zoom/drill-down:** Animated transitions with layout recalculated for focused subtree.

#### 3. The Tree View (Hierarchy)
*   A traditional folder view (like File Explorer).
*   **Lazy Loaded:** The frontend only requests the contents of the currently expanded folder from the Rust backend.

#### 4. The File View (Top Lists)
*   A flat list of the largest files on the drive.
*   **Virtual Scrolling:** Uses `TanStack Virtual` to render only the visible rows, allowing for lists of millions of files without UI lag.

#### 5. File Operations
*   **Recycle:** Move files to Windows Recycle Bin (via `SHFileOperation`).
*   **Permanent Delete:** Direct filesystem removal.
*   **Context Menu:** "Show in Explorer", "Copy Path", "Properties".

### 5. Known Limitations & Edge Cases

#### NTFS-Only Core Engine
*   **Primary Scan Method:** Direct MFT parsing (NTFS volumes only).
*   **Fallback Strategy:** For non-NTFS volumes (exFAT, FAT32, ReFS, network drives), use `FindFirstFileEx` with `FindExInfoBasic | FIND_FIRST_EX_LARGE_FETCH`.
*   **User Impact:** Scans on non-NTFS volumes will be slower but still functional.

#### Administrator Privileges Required
*   Reading the raw `$MFT` requires **Administrator elevation**.
*   **Strategy:**
    *   On startup, check for admin privileges.
    *   If not elevated, show UAC prompt via Tauri's `shell` API.
    *   If user declines, fall back to `FindFirstFileEx` with a warning that scan will be slower.

#### Edge Cases in NTFS
*   **Junction Points / Symlinks / Reparse Points:** Detected via file attributes and excluded from traversal to prevent infinite loops.
*   **Locked Files:** MFT references may point to files the process can't access (BitLocker, system files). These are logged as "inaccessible" but size is still counted.
*   **USN Journal:** For incremental rescans after file deletions, Phase 5 will leverage the NTFS USN Journal for live change tracking.

#### Index Limits
*   **Maximum Files:** `u32` indices support up to **4.29 billion files** (far beyond realistic use cases).
*   **String Arena Capacity:** `u32 name_offset` caps total string storage at **4GB**.
    *   With an average filename of ~20 bytes, this supports **~200 million files**.
    *   If needed in the future, `name_offset` can be upgraded to `u48` or `u64`.

### 6. Roadmap

*   **Phase 1: The Skeleton**
    *   Set up Tauri + Svelte + Rust project structure.
    *   Implement basic MFT reading (raw bytes to struct).
*   **Phase 2: The Data Model**
    *   Implement the Flat Tree architecture.
    *   Implement the String Arena.
    *   Verify RAM usage is <500MB for large drives.
*   **Phase 3: The Visuals**
    *   Implement Canvas Treemap rendering with cushion shading.
    *   Connect Rust data to Svelte UI.
    *   Add dark mode theme (default).
*   **Phase 4: Interaction**
    *   Add scan progress reporting and cancellation.
    *   Add deletion logic with confirmation dialogs.
    *   Add file context menus.
    *   Implement UAC elevation prompt on startup.
*   **Phase 5: Live Updates & Multi-Drive**
    *   Integrate USN Journal for incremental rescans.
    *   Add multi-drive support with unified comparison view.
    *   Implement fallback scan path for non-NTFS volumes.
*   **Phase 6: Power Features**
    *   Duplicate file finder (group by size + hash).
    *   File type heatmap (color tiles by extension category).
    *   Stale file detector (files untouched for >1 year).
    *   Export scan results to CSV/JSON.
    *   Benchmarking suite vs. WizTree/WinDirStat.

### 7. Future Features (Post-MVP)

| Feature | Description | Impact |
|---------|-------------|--------|
| **Duplicate Finder** | Group files by `(size, hash)` and offer batch cleanup. | Reclaim wasted space. |
| **File Type Heatmap** | Color treemap tiles by category (media, docs, code, system). | Instant visual insight into drive composition. |
| **Stale File Detector** | Flag files with `last_access_time > 1 year ago`. | Identify archival candidates. |
| **Export Functionality** | Export scan results to CSV/JSON for scripting/analysis. | Power-user workflows. |
| **USN Journal Integration** | Incremental rescans using NTFS change journal. | Near-instant refresh after edits. |
| **Multi-Drive Dashboard** | Scan all drives and compare usage side-by-side. | Whole-system visibility. |
| **Benchmarking** | Publish `BENCHMARKS.md` comparing Oxide vs. competitors. | Marketing & credibility. |

### 8. Getting Started (For Contributors)

*   **Prerequisites:**
    *   Rust (latest stable)
    *   Node.js & PNPM
    *   Visual Studio C++ Build Tools (for Windows API linking)

*   **Running the App:**
    ```bash
    # Install frontend dependencies
    pnpm install

    # Run in development mode
    # Note: Must run terminal as Administrator to access MFT!
    pnpm tauri dev
    ```

***

**License:** MIT / Apache 2.0
**Maintainer:** Keethesh Mootoosamy