# Oxide Benchmarks

This file records repeatable scan measurements for Oxide. Treat these as engineering baselines, not marketing claims, unless the environment and comparison method are fully documented.

## How To Capture A Run

Use the same machine and drive for repeated comparisons when possible.

1. Start Oxide from an elevated terminal:

   ```powershell
   pnpm tauri dev
   ```

2. Scan one NTFS volume and wait until the status reads complete.
3. Record memory after scan settles for a few seconds.
4. If comparing against another tool, scan the same volume without changing the file tree between runs.

## Fields

| Field | Meaning |
| --- | --- |
| Date | Date the run was captured |
| Build | App build or git commit if available |
| Mode | `mft` or `filesystem` |
| Elevated | Whether the app had administrator privileges |
| Files | Files reported by Oxide |
| Folders | Folders reported by Oxide |
| Total nodes | Files plus folders |
| Data size | Total scanned bytes displayed by Oxide |
| Reported duration | Tool-reported scan duration |
| Time to interactive | Measured time from starting the scan until the tree is usable |
| Memory after scan | Resident memory after the scan completed and settled |
| Nodes/sec | Total nodes divided by duration |
| Files/sec | Files divided by duration |
| Bytes/node | Memory after scan divided by total nodes |
| Notes | Hardware, drive type, cache state, or comparison caveats |

## Results

| Date | Build | Mode | Elevated | Files | Folders | Total nodes | Data size | Reported duration | Time to interactive | Memory after scan | Nodes/sec | Files/sec | Bytes/node | Notes |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 2026-04-25 | local dev | mft | yes | 4,041,909 | 733,450 | 4,775,359 | 369 GB | 20 s | 20 s | 430 MB | 238,768 | 202,095 | ~94.5 | Post-optimization baseline; total includes backend scan and initial UI rendering; memory unit assumed MiB for bytes/node estimate |
| 2026-04-25 | WizTree | mft-like NTFS metadata scan | assumed yes | 4,041,909 | 733,450 | 4,775,359 | 369 GB | 13.41 s | 21 s | 2.7 GB | 356,104 | 301,410 | ~592.7 | Self-reported scan time plus manual observed time until tree became visible; memory unit assumed GiB for bytes/node estimate |

## Current Baseline

The active Oxide baseline scanned 4.78 million file-system nodes in 20 seconds from scan start through initial UI rendering and settled at about 430 MB of RAM. The first WizTree comparison on the same reported file set completed in 13.41 seconds by its own timer, became usable after about 21 seconds wall-clock, and settled at about 2.7 GB of RAM.

Calculated from the reported numbers:

```text
Oxide:   4,775,359 nodes / 20.00 s = 238,768 nodes/sec
Oxide:   4,041,909 files / 20.00 s = 202,095 files/sec
Oxide:   430 MiB / 4,775,359 nodes = ~94.5 bytes/node
WizTree: 4,775,359 nodes / 13.41 s = 356,104 nodes/sec
WizTree: 4,041,909 files / 13.41 s = 301,410 files/sec
WizTree: 2.7 GiB / 4,775,359 nodes = ~592.7 bytes/node
```

At these measurements, WizTree is about 1.49x faster by its self-reported scan time, while Oxide is slightly faster by observed time to usable UI and uses about 6.3x less RAM after scan.

## Comparison Rules

When comparing with WizTree, WinDirStat, or another analyzer:

- Use the same volume and avoid file changes between runs.
- Record whether the drive was warm from a previous scan.
- Record whether each app was elevated.
- Capture both scan duration and post-scan memory.
- If the tool exposes its own timer, record that separately from wall-clock time to usable UI.
- Prefer three runs per tool and report median values.
- Keep UI interactions out of scan timing unless explicitly measuring end-to-interactive behavior.
