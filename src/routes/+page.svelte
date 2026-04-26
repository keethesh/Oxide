<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import FileList from "$lib/components/FileList.svelte";
  import Treemap from "$lib/components/Treemap.svelte";
  import TreeView from "$lib/components/TreeView.svelte";
  import type {
    DriveInfo,
    FallbackReason,
    LaunchScanRequest,
    PrepareScanResult,
    ScanMode,
    ScanProgress,
    ScanResult
  } from "$lib/types";

  let drives = $state<DriveInfo[]>([]);
  let selectedDrive = $state("");
  let status = $state("Choose an NTFS drive to begin.");
  let activeTab = $state<"treemap" | "list">("treemap");
  let progress = $state<ScanProgress>(idleProgress());
  let scanResult = $state<ScanResult | null>(null);
  let rootId = $state(0);
  let breadcrumbPath = $state<[number, string][]>([]);
  let isScanning = $state(false);
  let drivesLoading = $state(true);
  let scanStartedAt = $state<number | null>(null);
  let liveDurationMs = $state(0);
  const tauriAvailable = isTauri();

  async function loadDrives() {
    if (!tauriAvailable) {
      drives = [];
      selectedDrive = "";
      status = "Browser preview mode. Open the Tauri app to scan local drives.";
      drivesLoading = false;
      return;
    }

    drivesLoading = true;
    try {
      drives = await invoke<DriveInfo[]>("list_drives");
      selectedDrive = drives.find((drive) => drive.supported)?.letter ?? drives[0]?.letter ?? "";
      status = drives.length
        ? "Choose an NTFS drive to begin."
        : "No local drives were found.";
    } catch (error) {
      status = `Failed to list drives: ${error}`;
    } finally {
      drivesLoading = false;
    }
  }

  async function scan(requestedDrive = selectedDriveInfo?.letter ?? selectedDrive) {
    if (!tauriAvailable) {
      status = "Scan is only available in the Tauri desktop app.";
      return;
    }

    const drive = drives.find((candidate) => candidate.letter === requestedDrive) ?? null;
    if (!drive?.supported || isScanning) {
      return;
    }

    isScanning = true;
    scanStartedAt = Date.now();
    liveDurationMs = 0;
    try {
      status = `Preparing scan for ${drive.letter}...`;
      const preparation = await invoke<PrepareScanResult>("prepare_scan", {
        driveLetter: drive.letter
      });

      if (preparation.action === "relaunching") {
        progress = idleProgress("Relaunching as administrator");
        status = `Relaunching ${preparation.pending_drive ?? drive.letter} as administrator...`;
        return;
      }

      const mode = preparation.mode;
      if (!mode) {
        throw new Error("prepare_scan returned no scan mode");
      }

      resetScanState();
      progress = idleProgress(initialPhase(mode), mode, preparation.fallback_reason);
      status = preparation.fallback_reason
        ? `Using slower fallback scan for ${drive.letter}: ${fallbackDescription(preparation.fallback_reason)}`
        : mode === "mft"
          ? `Starting fast MFT scan on ${drive.letter}...`
          : `Starting filesystem scan on ${drive.letter}...`;

      const result = await invoke<ScanResult>("scan_drive", {
        driveLetter: drive.letter,
        mode
      });

      scanResult = result;
      liveDurationMs = result.duration_ms;
      console.info("[oxide] scan profile", result.timings);
      rootId = result.root_id;
      status = result.fallback_reason
        ? `Scan complete for ${result.drive_letter} using ${modeLabel(result.scan_mode)} after fallback.`
        : `Scan complete for ${result.drive_letter} using ${modeLabel(result.scan_mode)}.`;
    } catch (error) {
      status = `Scan failed: ${error}`;
    } finally {
      isScanning = false;
      scanStartedAt = null;
    }
  }

  async function updateBreadcrumbs() {
    if (!tauriAvailable || !scanResult) {
      breadcrumbPath = [];
      return;
    }

    try {
      breadcrumbPath = await invoke<[number, string][]>("get_file_path", { id: rootId });
    } catch (error) {
      status = `Failed to load breadcrumb: ${error}`;
    }
  }

  function handleNavigate(id: number) {
    rootId = id;
  }

  function resetScanState() {
    scanResult = null;
    rootId = 0;
    breadcrumbPath = [];
    activeTab = "treemap";
  }

  $effect(() => {
    const currentRoot = rootId;
    const currentScanRoot = scanResult;
    if (currentScanRoot) {
      updateBreadcrumbs();
    }
  });

  $effect(() => {
    if (!isScanning || scanStartedAt === null) {
      return;
    }

    const startedAt = scanStartedAt;
    let frame = 0;
    const updateDuration = () => {
      liveDurationMs = Math.max(liveDurationMs, Date.now() - startedAt);
      frame = window.requestAnimationFrame(updateDuration);
    };

    updateDuration();

    return () => {
      window.cancelAnimationFrame(frame);
    };
  });

  onMount(() => {
    let cleanup = () => {};

    if (tauriAvailable) {
      const unlisten = listen<ScanProgress>("scan-progress", (event) => {
        progress = event.payload;
      });
      cleanup = () => {
        void unlisten.then((fn) => fn());
      };
    }

    void (async () => {
      await loadDrives();

      if (!tauriAvailable) {
        return;
      }

      try {
        const launchRequest = await invoke<LaunchScanRequest>("get_launch_scan_request");
        if (launchRequest.drive_letter) {
          selectedDrive = launchRequest.drive_letter;
          await scan(launchRequest.drive_letter);
        }
      } catch (error) {
        status = `Failed to read launch request: ${error}`;
      }
    })();

    return () => {
      cleanup();
    };
  });

  function idleProgress(
    phase = "Idle",
    scanMode: ScanMode | null = null,
    fallbackReason: FallbackReason | null = null
  ): ScanProgress {
    return {
      files_scanned: 0,
      dirs_scanned: 0,
      bytes_scanned: 0,
      phase,
      done: false,
      scan_mode: scanMode,
      fallback_reason: fallbackReason,
      duration_ms: 0
    };
  }

  function initialPhase(mode: ScanMode): string {
    return mode === "mft" ? "Reading MFT" : "Walking filesystem";
  }

  function formatSize(bytes: number): string {
    if (bytes === 0) return "0 B";
    const units = ["B", "KB", "MB", "GB", "TB"];
    let value = bytes;
    let unitIndex = 0;
    while (value >= 1024 && unitIndex < units.length - 1) {
      value /= 1024;
      unitIndex += 1;
    }
    return `${value.toFixed(value >= 10 || unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
  }

  function modeLabel(mode: ScanMode | null | undefined): string {
    switch (mode) {
      case "mft":
        return "fast MFT scan";
      case "filesystem":
        return "filesystem scan";
      default:
        return "idle";
    }
  }

  function fallbackDescription(reason: FallbackReason | null | undefined): string {
    switch (reason) {
      case "uac_declined":
        return "administrator access was declined";
      case "mft_probe_timeout":
        return "the MFT probe timed out";
      case "mft_read_error":
        return "raw MFT reads failed";
      case "mft_parse_error":
        return "the MFT data could not be parsed";
      case "mft_access_denied":
        return "raw volume access was denied";
      default:
        return "";
    }
  }

  function formatDuration(durationMs: number, precise = false): string {
    if (durationMs < 1000) {
      return `${durationMs} ms`;
    }

    const seconds = durationMs / 1000;
    if (precise) {
      return `${seconds.toFixed(seconds >= 60 ? 0 : 1)} s`;
    }

    return `${seconds.toFixed(seconds >= 10 ? 0 : 1)} s`;
  }

  function formatProfile(result: ScanResult | null): string {
    if (!result) {
      return "";
    }

    const timings = result.timings;
    return `Profile: scan ${formatDuration(timings.scan_ms)} · aggregate ${formatDuration(timings.aggregate_ms)} · largest-file index ${formatDuration(timings.largest_files_ms)} · store ${formatDuration(timings.store_ms)} · total ${formatDuration(timings.total_ms)}`;
  }

  async function exportSnapshot() {
    const snapshot = [
      `Oxide scan snapshot`,
      `Drive: ${scanResult?.drive_letter ?? selectedDriveInfo?.letter ?? (selectedDrive || "none")}`,
      `Status: ${status}`,
      `Mode: ${scanModeSummary}`,
      `Files: ${(scanResult?.files_scanned ?? progress.files_scanned).toLocaleString()}`,
      `Folders: ${(scanResult?.dirs_scanned ?? progress.dirs_scanned).toLocaleString()}`,
      `Size: ${formatSize(scanResult?.bytes_scanned ?? progress.bytes_scanned)}`,
      `Duration: ${visibleDurationLabel}`,
      profileSummary
    ].filter(Boolean).join("\n");

    try {
      await navigator.clipboard?.writeText(snapshot);
      status = "Scan snapshot copied to clipboard.";
    } catch {
      status = snapshot;
    }
  }

  const selectedDriveInfo = $derived(
    drives.find((drive) => drive.letter === selectedDrive) ?? null
  );
  const visibleDuration = $derived(
    isScanning ? Math.max(progress.duration_ms, liveDurationMs) : (scanResult?.duration_ms ?? progress.duration_ms)
  );
  const visibleDurationLabel = $derived(formatDuration(visibleDuration, isScanning));
  const currentScanMode = $derived(scanResult?.scan_mode ?? progress.scan_mode ?? null);
  const currentFallbackReason = $derived(
    scanResult?.fallback_reason ?? progress.fallback_reason ?? null
  );
  const scanModeSummary = $derived(
    currentScanMode ? modeLabel(currentScanMode) : "No active scan mode"
  );
  const fallbackSummary = $derived(
    currentFallbackReason ? fallbackDescription(currentFallbackReason) : ""
  );
  const profileSummary = $derived(formatProfile(scanResult));
  const totalItemsScanned = $derived(progress.files_scanned + progress.dirs_scanned);
  const scanActivityLabel = $derived(
    isScanning
      ? `${totalItemsScanned.toLocaleString()} entries indexed`
      : scanResult
        ? `${(scanResult.files_scanned + scanResult.dirs_scanned).toLocaleString()} entries mapped`
        : "Ready"
  );
  const activeFiles = $derived(scanResult?.files_scanned ?? progress.files_scanned);
  const activeDirs = $derived(scanResult?.dirs_scanned ?? progress.dirs_scanned);
  const activeBytes = $derived(scanResult?.bytes_scanned ?? progress.bytes_scanned);
  const scanProgressPercent = $derived.by(() => {
    if (scanResult) {
      return 100;
    }

    if (!isScanning) {
      return 0;
    }

    const elapsedSeconds = liveDurationMs / 1000;
    const expectedScanSeconds = currentScanMode === "filesystem" ? 55 : 24;
    const normalized = Math.min(1, elapsedSeconds / expectedScanSeconds);
    const eased = 1 - (1 - normalized) ** 2.4;
    return Math.min(94, Math.max(4, eased * 94));
  });
  const scanProgressStyle = $derived(`width: ${scanProgressPercent.toFixed(1)}%`);
  const selectedDriveLabel = $derived(
    selectedDriveInfo
      ? `${selectedDriveInfo.label} [${selectedDriveInfo.filesystem}]`
      : selectedDrive || "No drive selected"
  );
  const readyState = $derived(
    drivesLoading
      ? "Detecting volumes"
      : isScanning
        ? "Live scan"
        : scanResult
          ? "Scan complete"
          : tauriAvailable
            ? "Ready"
            : "Preview mode"
  );
</script>

<svelte:head>
  <title>Oxide</title>
</svelte:head>

<main class="app-shell">
  <nav class="rail" aria-label="Primary views">
    <img class="rail-brand" src="/logo.png" alt="Oxide" />
    <button
      class:active={activeTab === "treemap"}
      class="rail-btn"
      title="Treemap"
      aria-label="Treemap"
      onclick={() => (activeTab = "treemap")}
    >
      <span class="icon-grid" aria-hidden="true"></span>
    </button>
    <button
      class:active={activeTab === "list"}
      class="rail-btn"
      title="Largest files"
      aria-label="Largest files"
      onclick={() => (activeTab = "list")}
    >
      <span class="icon-list" aria-hidden="true"></span>
    </button>
    <span class="rail-divider" aria-hidden="true"></span>
    <button class="rail-btn" title="Copy scan snapshot" aria-label="Copy scan snapshot" onclick={exportSnapshot}>
      <span class="icon-export" aria-hidden="true"></span>
    </button>
  </nav>

  <header class="hud">
    <div class="hud-left">
      <div>
        <h1>Space Observatory</h1>
        <span>{selectedDriveLabel}</span>
      </div>
      <span class="status-pill">
        <span class:active={isScanning} class="status-dot" aria-hidden="true"></span>
        {readyState}
      </span>
    </div>

    <div class="hud-right">
      <label class="drive-picker">
        <span class="visually-hidden">Drive</span>
        <span class="select-shell">
          <select bind:value={selectedDrive} disabled={drivesLoading || isScanning || !drives.length}>
            {#each drives as drive}
              <option value={drive.letter}>
                {drive.label} [{drive.filesystem}]
              </option>
            {/each}
          </select>
        </span>
      </label>
      <button class="hud-btn ghost" onclick={exportSnapshot}>Export</button>
      <button
        class:scanning={isScanning}
        class="hud-btn primary scan-action"
        disabled={!selectedDriveInfo?.supported || isScanning || drivesLoading}
        onclick={() => scan()}
      >
        <svg
          class:active={isScanning}
          class="scan-icon"
          aria-hidden="true"
          viewBox="0 0 24 24"
          fill="none"
        >
          <path
            d="M20 12a8 8 0 1 1-2.34-5.66"
            stroke="currentColor"
            stroke-width="2.4"
            stroke-linecap="round"
          />
          <path
            d="M20 4v6h-6"
            stroke="currentColor"
            stroke-width="2.4"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
        {isScanning ? "Scanning" : "Scan"}
      </button>
    </div>
  </header>

  <section class="canvas-zone" aria-label="Disk workspace">
    {#if drivesLoading}
      <section class="empty-state">Detecting local volumes...</section>
    {:else if !drives.length}
      <section class="empty-state">
        {#if tauriAvailable}
          No local drives were detected on this system.
        {:else}
          Browser preview mode is running without the Tauri bridge.
        {/if}
      </section>
    {:else if selectedDriveInfo && !selectedDriveInfo.supported}
      <section class="empty-state">
        <strong>{selectedDriveInfo.letter} is {selectedDriveInfo.filesystem}</strong>
        <span>Oxide currently scans NTFS volumes.</span>
      </section>
    {:else if isScanning}
      <section class="scan-board" aria-label="Scan in progress">
        <div class="scan-core">
          <span class="status-pill">
            <span class:active={isScanning} class="status-dot" aria-hidden="true"></span>
            {progress.phase}
          </span>
          <strong>{selectedDriveInfo?.letter ?? selectedDrive} is being mapped</strong>
          <span>{scanActivityLabel} · {formatSize(activeBytes)} · {visibleDurationLabel}</span>
          <div class="scan-meter">
            <span style={scanProgressStyle}></span>
          </div>
        </div>
      </section>
    {:else if scanResult}
      <div class="workspace">
        <section class="visual-stage" aria-label="Disk visualization">
          <div class="stage-toolbar">
            <div class="stage-context">
              <span>{activeTab === "treemap" ? `Space map - ${scanResult.drive_letter}` : "Largest files"}</span>
              <div class="breadcrumb">
                {#each breadcrumbPath as [id, name], index (id)}
                  {#if index > 0}
                    <span class="sep">/</span>
                  {/if}
                  <button class="bc-item" onclick={() => (rootId = id)}>{name}</button>
                {/each}
              </div>
            </div>

            <nav class="view-tabs" aria-label="View mode">
              <button class:active={activeTab === "treemap"} onclick={() => (activeTab = "treemap")}>
                Treemap
              </button>
              <button class:active={activeTab === "list"} onclick={() => (activeTab = "list")}>
                Files
              </button>
            </nav>
          </div>

          <div class="visual-content">
            {#if activeTab === "treemap"}
              <Treemap {rootId} onNavigate={handleNavigate} />
            {:else}
              <FileList scanLoaded={true} {rootId} onNavigate={handleNavigate} compact={false} />
            {/if}
          </div>

          <div class="hint">Click a block to drill in · Use breadcrumbs to climb back</div>
        </section>

        <aside class="analysis-rail" aria-label="Disk analysis tools">
          <section class="navigator" aria-label="Folder navigator">
            <TreeView
              scanLoaded={true}
              scanRootId={scanResult.root_id}
              selectedId={rootId}
              onSelect={handleNavigate}
            />
          </section>

          <section class="inspector" aria-label="Largest files">
            <FileList scanLoaded={true} {rootId} onNavigate={handleNavigate} compact />
          </section>
        </aside>
      </div>
    {:else}
      <section class="start-board">
        <div class="start-copy">
          <img class="start-logo" src="/logo.png" alt="" aria-hidden="true" />
          <div>
            <strong>Choose a drive to inspect</strong>
            <span>Map storage pressure first, then drill into folders and heavy files.</span>
          </div>
        </div>

        <div class="drive-board" aria-label="Available drives">
          {#each drives as drive}
            <button
              class:selected={selectedDrive === drive.letter}
              class:unsupported={!drive.supported}
              disabled={!drive.supported || isScanning}
              onclick={() => {
                selectedDrive = drive.letter;
              }}
            >
              <span>
                <strong>{drive.letter}</strong>
                <small>{drive.label}</small>
              </span>
              <em>{drive.supported ? drive.filesystem : `${drive.filesystem} unsupported`}</em>
            </button>
          {/each}
        </div>
      </section>
    {/if}
  </section>

  <section class="drawer" aria-label="Scan summary">
    <div class="drawer-stat">
      <span>Files</span>
      <strong>{activeFiles.toLocaleString()}</strong>
    </div>
    <i></i>
    <div class="drawer-stat">
      <span>Folders</span>
      <strong>{activeDirs.toLocaleString()}</strong>
    </div>
    <i></i>
    <div class="drawer-stat">
      <span>Indexed</span>
      <strong>{formatSize(activeBytes)}</strong>
    </div>
    <i></i>
    <div class="drawer-stat">
      <span>Scan time</span>
      <strong>{visibleDurationLabel}</strong>
    </div>
    <div class="drawer-progress">
      <div>
        <span>{progress.phase}</span>
        <span>{scanActivityLabel}</span>
      </div>
      <div class:active={isScanning} class="activity-line">
        <span style={scanProgressStyle}></span>
      </div>
    </div>
  </section>

  <footer class="telemetry">
    <div>
      <span>MFT</span><strong>{scanResult ? formatDuration(scanResult.timings.scan_ms) : "--"}</strong>
      <span>Aggregate</span><strong>{scanResult ? formatDuration(scanResult.timings.aggregate_ms) : "--"}</strong>
      <span>Index</span><strong>{scanResult ? formatDuration(scanResult.timings.largest_files_ms) : "--"}</strong>
    </div>
    <div>
      <span>Mode</span><strong>{fallbackSummary || scanModeSummary}</strong>
      <span>Status</span><strong>{status}</strong>
    </div>
  </footer>
</main>

<style>
  :global(body) {
    margin: 0;
    min-height: 100vh;
    overflow: hidden;
    background:
      linear-gradient(90deg, rgba(223, 245, 154, 0.024) 1px, transparent 1px),
      linear-gradient(180deg, rgba(223, 245, 154, 0.018) 1px, transparent 1px),
      radial-gradient(circle at 16% 12%, rgba(223, 245, 154, 0.08), transparent 22rem),
      linear-gradient(135deg, #0e1210 0%, #080b09 58%, #15110f 100%);
    background-size: 48px 48px, 48px 48px, auto, auto;
    color: #f1ece2;
    font-family: "Aptos", "Segoe UI Variable", "Segoe UI", sans-serif;
  }

  :global(button),
  :global(select) {
    font: inherit;
  }

  :global(button:focus-visible),
  :global(select:focus-visible) {
    outline: 2px solid #dff59a;
    outline-offset: 2px;
  }

  :global(::-webkit-scrollbar) {
    width: 8px;
    height: 8px;
  }

  :global(::-webkit-scrollbar-track) {
    background: transparent;
  }

  :global(::-webkit-scrollbar-thumb) {
    background: #343b2e;
    border: 2px solid #10130f;
    border-radius: 999px;
  }

  .app-shell {
    --bg-void: #0a0d0b;
    --bg-base: #0e1210;
    --bg-panel: rgba(21, 23, 18, 0.94);
    --bg-glass: rgba(14, 18, 16, 0.78);
    --surface-1: oklch(18% 0.015 125);
    --surface-2: oklch(22% 0.017 125);
    --surface-3: oklch(26% 0.019 115);
    --accent: #dff59a;
    --accent-2: #f2b16f;
    --warn: #ffb199;
    --text: #fbf6eb;
    --muted: #9a948a;
    --dim: #6b665e;
    --line: rgba(223, 245, 154, 0.08);
    --line-soft: rgba(223, 245, 154, 0.045);
    --ease: cubic-bezier(0.16, 1, 0.3, 1);
    display: grid;
    grid-template-rows: 52px minmax(0, 1fr) auto 32px;
    grid-template-columns: 48px minmax(0, 1fr);
    grid-template-areas:
      "rail hud"
      "rail canvas"
      "rail drawer"
      "rail telemetry";
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    background: var(--bg-void);
    color: #e8e2d6;
  }

  .rail {
    grid-area: rail;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 14px 0;
    border-right: 1px solid var(--line-soft);
    background: var(--bg-base);
    box-sizing: border-box;
  }

  .rail-brand {
    width: 32px;
    height: 32px;
    margin-bottom: 22px;
    border: 1px solid rgba(223, 245, 154, 0.22);
    border-radius: 8px;
    background: #090d0a;
    object-fit: cover;
    object-position: left center;
    box-shadow: 0 0 24px rgba(223, 245, 154, 0.18);
  }

  .rail-btn {
    position: relative;
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border: 0;
    border-radius: 10px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    transition: background 180ms var(--ease), color 180ms var(--ease), transform 180ms var(--ease);
  }

  .rail-btn:hover {
    background: var(--surface-1);
    color: #e8e2d6;
    transform: translateY(-1px);
  }

  .rail-btn.active {
    background: var(--surface-2);
    color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--line);
  }

  .rail-btn.active::before {
    content: "";
    position: absolute;
    left: -6px;
    width: 3px;
    height: 16px;
    border-radius: 0 2px 2px 0;
    background: var(--accent);
  }

  .rail-divider {
    width: 20px;
    height: 1px;
    margin: 8px 0;
    background: var(--line-soft);
  }

  .icon-grid,
  .icon-list,
  .icon-export {
    width: 17px;
    height: 17px;
    display: block;
  }

  .icon-grid {
    background:
      linear-gradient(currentColor 0 0) 0 0 / 7px 7px,
      linear-gradient(currentColor 0 0) 10px 0 / 7px 7px,
      linear-gradient(currentColor 0 0) 0 10px / 7px 7px,
      linear-gradient(currentColor 0 0) 10px 10px / 7px 7px;
    background-repeat: no-repeat;
  }

  .icon-list {
    background:
      linear-gradient(currentColor 0 0) 0 2px / 17px 2px,
      linear-gradient(currentColor 0 0) 0 8px / 17px 2px,
      linear-gradient(currentColor 0 0) 0 14px / 17px 2px;
    background-repeat: no-repeat;
  }

  .icon-export {
    border: 2px solid currentColor;
    border-top: 0;
    box-sizing: border-box;
  }

  .icon-export::before {
    content: "";
    display: block;
    width: 7px;
    height: 7px;
    margin: -1px auto 0;
    border-top: 2px solid currentColor;
    border-right: 2px solid currentColor;
    transform: rotate(-45deg);
  }

  .hud {
    grid-area: hud;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    padding: 0 20px;
    border-bottom: 1px solid var(--line-soft);
    background: var(--bg-glass);
    backdrop-filter: blur(20px) saturate(1.2);
    box-sizing: border-box;
  }

  .hud-left,
  .hud-right {
    display: flex;
    align-items: center;
    min-width: 0;
  }

  .hud-left {
    gap: 18px;
  }

  .hud-right {
    justify-content: flex-end;
    gap: 10px;
  }

  .hud h1 {
    margin: 0;
    color: var(--text);
    font-size: 0.84rem;
    font-weight: 760;
    line-height: 1.1;
  }

  .hud-left div {
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  .hud-left div > span {
    overflow: hidden;
    color: var(--muted);
    font-size: 0.68rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .status-pill {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    min-height: 24px;
    border: 1px solid rgba(223, 245, 154, 0.12);
    border-radius: 6px;
    background: rgba(223, 245, 154, 0.08);
    color: var(--accent);
    font-size: 0.68rem;
    font-weight: 700;
    padding: 0 10px;
    white-space: nowrap;
  }

  .status-dot {
    width: 6px;
    height: 6px;
    border-radius: 999px;
    background: var(--accent);
    box-shadow: 0 0 8px rgba(223, 245, 154, 0.55);
  }

  .status-dot.active {
    animation: breathe 1.7s var(--ease) infinite;
  }

  .drive-picker {
    display: block;
    width: min(30vw, 24rem);
    min-width: 13rem;
  }

  .select-shell {
    position: relative;
    display: block;
  }

  .select-shell::after {
    content: "";
    position: absolute;
    right: 12px;
    top: 50%;
    width: 7px;
    height: 7px;
    border-right: 2px solid var(--muted);
    border-bottom: 2px solid var(--muted);
    pointer-events: none;
    transform: translateY(-65%) rotate(45deg);
  }

  .hud select {
    width: 100%;
    appearance: none;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--surface-1);
    color: #e8e2d6;
    cursor: pointer;
    font-size: 0.75rem;
    font-weight: 650;
    padding: 0.45rem 2rem 0.45rem 0.75rem;
  }

  .hud-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    min-height: 32px;
    border-radius: 8px;
    cursor: pointer;
    font-size: 0.75rem;
    font-weight: 760;
    padding: 0 0.9rem;
    transition: background 180ms var(--ease), box-shadow 180ms var(--ease), transform 180ms var(--ease), color 180ms var(--ease);
  }

  .hud-btn.ghost {
    border: 1px solid var(--line);
    background: transparent;
    color: var(--muted);
  }

  .hud-btn.ghost:hover {
    background: var(--surface-1);
    color: #e8e2d6;
  }

  .hud-btn.primary {
    border: 0;
    background: var(--accent);
    color: #171b0b;
  }

  .hud-btn.primary:hover:not(:disabled) {
    box-shadow: 0 0 20px rgba(223, 245, 154, 0.28);
    transform: translateY(-1px);
  }

  .hud select:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }

  .hud-btn:disabled {
    cursor: not-allowed;
  }

  .hud-btn.primary:disabled:not(.scanning) {
    opacity: 0.55;
  }

  .scan-action.scanning {
    opacity: 1;
    box-shadow: 0 0 20px rgba(223, 245, 154, 0.22);
  }

  .canvas-zone {
    grid-area: canvas;
    position: relative;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background:
      linear-gradient(rgba(223, 245, 154, 0.03) 1px, transparent 1px),
      linear-gradient(90deg, rgba(223, 245, 154, 0.025) 1px, transparent 1px),
      var(--bg-void);
    background-size: 40px 40px;
  }

  .workspace {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(20rem, 23rem);
    width: 100%;
    height: 100%;
    min-height: 0;
  }

  .visual-stage {
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }

  .stage-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    min-height: 54px;
    padding: 12px 18px 8px;
    box-sizing: border-box;
  }

  .stage-context {
    display: grid;
    gap: 4px;
    min-width: 0;
  }

  .stage-context > span {
    color: var(--muted);
    font-size: 0.64rem;
    font-weight: 850;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .breadcrumb {
    display: flex;
    align-items: center;
    gap: 3px;
    min-width: 0;
  }

  .bc-item {
    max-width: 12rem;
    overflow: hidden;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: #e8e2d6;
    cursor: pointer;
    padding: 0.12rem 0.35rem;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: color 160ms var(--ease), background 160ms var(--ease);
  }

  .bc-item:hover {
    background: rgba(223, 245, 154, 0.08);
    color: var(--accent);
  }

  .sep {
    color: var(--dim);
  }

  .view-tabs {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--surface-1);
    padding: 4px;
  }

  .view-tabs button {
    min-width: 4.2rem;
    min-height: 28px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    font-size: 0.74rem;
  }

  .view-tabs button:hover,
  .view-tabs button.active {
    color: var(--text);
  }

  .view-tabs button.active {
    background: var(--surface-3);
    font-weight: 800;
  }

  .visual-content {
    flex: 1;
    min-height: 0;
    padding: 0 18px 18px;
    box-sizing: border-box;
  }

  .visual-content > :global(*) {
    min-height: 0;
  }

  .analysis-rail {
    display: grid;
    grid-template-rows: minmax(13rem, 0.95fr) minmax(14rem, 1fr);
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    border-left: 1px solid var(--line);
    background: var(--bg-panel);
    backdrop-filter: blur(16px);
  }

  .navigator,
  .inspector {
    min-height: 0;
    overflow: hidden;
  }

  .inspector {
    border-top: 1px solid var(--line-soft);
    padding: 14px 16px;
    box-sizing: border-box;
  }

  .hint {
    position: absolute;
    left: 18px;
    bottom: 18px;
    z-index: 2;
    border: 1px solid var(--line-soft);
    border-radius: 8px;
    background: rgba(14, 18, 16, 0.72);
    color: var(--muted);
    font-size: 0.72rem;
    padding: 0.45rem 0.7rem;
    pointer-events: none;
  }

  .empty-state,
  .scan-board,
  .start-board {
    width: 100%;
    height: 100%;
    min-height: 0;
  }

  .empty-state {
    display: grid;
    place-content: center;
    gap: 0.45rem;
    color: var(--muted);
    padding: 2rem;
    text-align: center;
    box-sizing: border-box;
  }

  .empty-state strong {
    color: var(--text);
  }

  .scan-board {
    display: grid;
    place-items: center;
    padding: clamp(1.5rem, 4vw, 4rem);
    box-sizing: border-box;
  }

  .scan-core {
    display: grid;
    gap: 12px;
    width: min(38rem, 100%);
    border: 1px solid var(--line);
    border-radius: 12px;
    background:
      linear-gradient(135deg, rgba(223, 245, 154, 0.085), transparent 52%),
      rgba(14, 18, 16, 0.72);
    box-shadow: 0 24px 70px rgba(0, 0, 0, 0.28);
    padding: clamp(1.25rem, 3vw, 2rem);
    backdrop-filter: blur(14px);
  }

  .scan-core .status-pill {
    justify-self: start;
  }

  .scan-core strong {
    color: var(--text);
    font-size: clamp(1.35rem, 2.6vw, 2.4rem);
    font-weight: 820;
    line-height: 1.05;
  }

  .scan-core > span:not(.status-pill) {
    color: var(--muted);
  }

  .scan-meter {
    height: 8px;
    overflow: hidden;
    border-radius: 999px;
    background: rgba(223, 245, 154, 0.08);
  }

  .scan-meter span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background:
      linear-gradient(90deg, rgba(255, 255, 255, 0), rgba(255, 255, 255, 0.28), rgba(255, 255, 255, 0)) 0 0 / 72px 100%,
      linear-gradient(90deg, var(--accent), var(--accent-2));
    transition: width 520ms var(--ease);
    animation: meter-sheen 1.6s linear infinite;
  }

  .start-board {
    display: grid;
    grid-template-columns: minmax(19rem, 0.82fr) minmax(21rem, 1fr);
    overflow: hidden;
  }

  .start-copy {
    display: grid;
    align-content: center;
    gap: 24px;
    border-right: 1px solid var(--line);
    background: rgba(21, 23, 18, 0.62);
    padding: clamp(1.5rem, 4vw, 4rem);
  }

  .start-logo {
    width: min(360px, 82%);
    max-height: 180px;
    object-fit: contain;
    object-position: left center;
    filter: drop-shadow(0 0 32px rgba(223, 245, 154, 0.16));
  }

  .start-copy div {
    display: grid;
    gap: 0.65rem;
    max-width: 31rem;
  }

  .start-copy strong {
    color: var(--text);
    font-size: clamp(1.65rem, 3vw, 3rem);
    font-weight: 820;
    line-height: 1.02;
  }

  .start-copy span {
    color: var(--muted);
    line-height: 1.55;
  }

  .drive-board {
    display: grid;
    align-content: start;
    gap: 10px;
    overflow: auto;
    padding: clamp(1rem, 2.4vw, 1.5rem);
  }

  .drive-board button {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    min-height: 68px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--surface-1);
    color: #ebe4d8;
    cursor: pointer;
    padding: 0.85rem 1rem;
    text-align: left;
    transition: border-color 180ms var(--ease), background 180ms var(--ease), transform 180ms var(--ease);
  }

  .drive-board button:hover:not(:disabled),
  .drive-board button.selected {
    border-color: rgba(223, 245, 154, 0.34);
    background: var(--surface-2);
    transform: translateY(-1px);
  }

  .drive-board button:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }

  .drive-board button span {
    display: grid;
    gap: 0.15rem;
    min-width: 0;
  }

  .drive-board button strong {
    color: var(--text);
    font-size: 1.05rem;
  }

  .drive-board small,
  .drive-board em {
    overflow: hidden;
    color: var(--muted);
    font-size: 0.78rem;
    font-style: normal;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .drive-board em {
    flex-shrink: 0;
    border: 1px solid var(--line);
    border-radius: 999px;
    padding: 0.22rem 0.55rem;
  }

  .drive-board button.selected em {
    border-color: rgba(223, 245, 154, 0.35);
    color: var(--accent);
  }

  .drawer {
    grid-area: drawer;
    display: grid;
    grid-template-columns: auto 1px auto 1px auto 1px auto minmax(12rem, 1fr);
    align-items: center;
    gap: 18px;
    min-height: 58px;
    border-top: 1px solid var(--line);
    background: var(--bg-panel);
    padding: 10px 20px;
    box-sizing: border-box;
  }

  .drawer i {
    width: 1px;
    height: 24px;
    background: var(--line-soft);
  }

  .drawer-stat {
    display: grid;
    gap: 2px;
    min-width: 4.25rem;
  }

  .drawer-stat span,
  .drawer-progress span,
  .telemetry span {
    color: var(--dim);
    font-size: 0.64rem;
    font-weight: 750;
    letter-spacing: 0.09em;
    text-transform: uppercase;
  }

  .drawer-stat strong {
    color: var(--text);
    font-size: 0.84rem;
    font-variant-numeric: tabular-nums;
  }

  .drawer-progress {
    display: grid;
    gap: 7px;
    min-width: 0;
  }

  .drawer-progress > div:first-child {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    min-width: 0;
  }

  .drawer-progress > div:first-child span:last-child {
    overflow: hidden;
    color: var(--muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .activity-line {
    height: 4px;
    overflow: hidden;
    border-radius: 999px;
    background: var(--surface-1);
  }

  .activity-line span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background:
      linear-gradient(90deg, rgba(255, 255, 255, 0), rgba(255, 255, 255, 0.32), rgba(255, 255, 255, 0)) 0 0 / 64px 100%,
      linear-gradient(90deg, var(--accent), var(--accent-2));
    transition: width 520ms var(--ease);
  }

  .activity-line.active span {
    animation: meter-sheen 1.35s linear infinite;
  }

  .telemetry {
    grid-area: telemetry;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    min-width: 0;
    border-top: 1px solid var(--line-soft);
    background: var(--bg-base);
    padding: 0 20px;
    box-sizing: border-box;
  }

  .telemetry div {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .telemetry strong {
    overflow: hidden;
    color: #d6cfc1;
    font-size: 0.65rem;
    font-variant-numeric: tabular-nums;
    font-weight: 650;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .telemetry div:last-child {
    justify-content: flex-end;
  }

  .scan-icon {
    width: 15px;
    height: 15px;
    flex: 0 0 auto;
    transform-origin: 50% 50%;
  }

  .scan-icon.active {
    animation: spin 850ms linear infinite;
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  @keyframes breathe {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.42;
    }
  }

  @keyframes meter-sheen {
    to {
      background-position: 140px 0, 0 0;
    }
  }

  @media (max-width: 1120px) {
    .workspace {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: minmax(28rem, 1fr) minmax(17rem, 0.5fr);
    }

    .analysis-rail {
      grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
      grid-template-rows: 1fr;
      border-top: 1px solid var(--line);
      border-left: 0;
    }

    .inspector {
      border-top: 0;
      border-left: 1px solid var(--line-soft);
    }
  }

  @media (max-width: 820px) {
    :global(body) {
      overflow: auto;
    }

    .app-shell {
      grid-template-columns: 1fr;
      grid-template-rows: auto auto minmax(30rem, 1fr) auto auto;
      grid-template-areas:
        "rail"
        "hud"
        "canvas"
        "drawer"
        "telemetry";
      height: auto;
      min-height: 100vh;
      overflow: visible;
    }

    .rail {
      flex-direction: row;
      justify-content: flex-start;
      padding: 8px 12px;
      border-right: 0;
      border-bottom: 1px solid var(--line-soft);
    }

    .rail-brand {
      margin: 0 12px 0 0;
    }

    .rail-btn.active::before {
      left: 50%;
      bottom: -8px;
      top: auto;
      width: 16px;
      height: 3px;
      border-radius: 2px 2px 0 0;
      transform: translateX(-50%);
    }

    .rail-divider {
      width: 1px;
      height: 20px;
      margin: 0 4px;
    }

    .hud,
    .hud-left,
    .hud-right {
      align-items: stretch;
      flex-direction: column;
    }

    .hud {
      padding: 12px;
    }

    .drive-picker,
    .hud-btn {
      width: 100%;
    }

    .workspace,
    .start-board,
    .analysis-rail {
      grid-template-columns: 1fr;
      grid-template-rows: auto auto;
    }

    .canvas-zone {
      min-height: 34rem;
    }

    .visual-stage {
      min-height: 30rem;
    }

    .analysis-rail {
      min-height: 34rem;
    }

    .inspector,
    .start-copy {
      border-left: 0;
      border-right: 0;
      border-top: 1px solid var(--line-soft);
    }

    .drawer {
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 12px;
    }

    .drawer i {
      display: none;
    }

    .drawer-progress {
      grid-column: 1 / -1;
    }

    .telemetry,
    .telemetry div {
      align-items: flex-start;
      flex-direction: column;
    }
  }
</style>
