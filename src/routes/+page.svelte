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
</script>

<svelte:head>
  <title>Oxide</title>
</svelte:head>

<main class="shell">
  <header class="top-bar">
    <div class="brand">
      <img class="brand-mark" src="/logo.png" alt="Oxide logo" />
      <div class="brand-copy">
        <h1>Oxide</h1>
        <span>NTFS disk space analyzer</span>
      </div>
    </div>

    <div class="top-controls">
      <label class="drive-picker">
        <span>Drive</span>
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

      <button
        class="scan-button"
        disabled={!selectedDriveInfo?.supported || isScanning || drivesLoading}
        onclick={() => scan()}
      >
        <span class:active={isScanning} class="button-icon" aria-hidden="true"></span>
        <span>{isScanning ? "Scanning" : "Scan drive"}</span>
      </button>
    </div>
  </header>

  <section class="status-banner">
    <div class="status-left">
      <span class:active={isScanning} class="status-dot" aria-hidden="true"></span>
      <div class="status-copy">
        <strong>{status}</strong>
        <span>{progress.phase} - {fallbackSummary || scanModeSummary}</span>
      {#if profileSummary}
          <span class="profile-line">{profileSummary}</span>
      {/if}
      </div>
    </div>

    <div class="metrics" aria-label="Scan metrics">
      <div>
        <span>Files</span>
        <strong>{progress.files_scanned.toLocaleString()}</strong>
      </div>
      <div>
        <span>Folders</span>
        <strong>{progress.dirs_scanned.toLocaleString()}</strong>
      </div>
      <div>
        <span>Size</span>
        <strong>{formatSize(progress.bytes_scanned)}</strong>
      </div>
      <div>
        <span>Time</span>
        <strong>{visibleDurationLabel}</strong>
      </div>
    </div>

    <div class:active={isScanning} class="activity-line" aria-label={scanActivityLabel}>
      <span></span>
    </div>
  </section>

  {#if drivesLoading}
    <section class="empty-state">Loading drives...</section>
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
      {selectedDriveInfo.letter} is formatted as {selectedDriveInfo.filesystem}. This MVP only supports NTFS volumes.
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
              Largest Files
            </button>
          </nav>
        </div>

        <div class="visual-content">
          {#if activeTab === "treemap"}
            <Treemap {rootId} onNavigate={handleNavigate} />
          {:else}
            <FileList scanLoaded={true} {rootId} onNavigate={handleNavigate} />
          {/if}
        </div>
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
          <FileList scanLoaded={true} {rootId} onNavigate={handleNavigate} />
        </section>
      </aside>
    </div>
  {:else}
    <section class="start-board">
      <div class="start-copy">
        <img class="start-logo" src="/logo.png" alt="" aria-hidden="true" />
        <div>
          <strong>Choose a drive to inspect</strong>
          <span>Start with a map-first view of storage pressure, then drill into folders and large files without loading the entire tree into the DOM.</span>
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

  .shell {
    --bg: #0e1210;
    --panel: #151712;
    --surface: oklch(20% 0.018 125);
    --surface-hover: oklch(24% 0.019 125);
    --surface-active: oklch(27% 0.02 115);
    --line: rgba(223, 245, 154, 0.08);
    --line-strong: rgba(223, 245, 154, 0.18);
    --text-muted: #aaa397;
    --text-dim: #a8a094;
    --text-soft: #d6cfc1;
    --accent: #dff59a;
    --accent-2: #f2b16f;
    --accent-ink: #20240c;
    --warn: #ffb299;
    --ease: cubic-bezier(0.16, 1, 0.3, 1);
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    height: 100vh;
    min-width: 0;
    overflow: hidden;
    background: rgba(8, 11, 9, 0.44);
  }

  .top-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    min-height: 66px;
    border-bottom: 1px solid var(--line);
    background:
      linear-gradient(180deg, rgba(255, 252, 239, 0.045), transparent),
      rgba(21, 23, 18, 0.96);
    padding: 0 24px;
    box-sizing: border-box;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 13px;
    min-width: 12rem;
  }

  .brand-mark {
    width: 42px;
    height: 42px;
    flex: 0 0 auto;
    object-fit: cover;
    object-position: left center;
    border: 1px solid rgba(223, 245, 154, 0.24);
    border-radius: 9px;
    background: #090d0a;
    box-shadow: 0 0 28px rgba(223, 245, 154, 0.14);
  }

  .brand-copy {
    display: grid;
    gap: 1px;
    min-width: 0;
  }

  .brand h1 {
    margin: 0;
    color: #fbf6eb;
    font-size: 1.02rem;
    font-weight: 780;
    letter-spacing: 0;
    line-height: 1.1;
  }

  .brand-copy span,
  .drive-picker span,
  .status-copy span,
  .metrics span {
    color: var(--text-muted);
    font-size: 0.72rem;
  }

  .top-controls {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 16px;
    min-width: min(40rem, 58vw);
  }

  .drive-picker {
    display: grid;
    grid-template-columns: auto minmax(14rem, 28rem);
    align-items: center;
    gap: 10px;
    min-width: 0;
    flex: 1;
  }

  .drive-picker > span:first-child {
    font-weight: 800;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .select-shell {
    position: relative;
    display: block;
    min-width: 0;
  }

  .select-shell::after {
    content: "";
    position: absolute;
    top: 50%;
    right: 13px;
    width: 7px;
    height: 7px;
    border-right: 2px solid var(--text-muted);
    border-bottom: 2px solid var(--text-muted);
    pointer-events: none;
    transform: translateY(-65%) rotate(45deg);
  }

  select {
    width: 100%;
    min-width: 0;
    appearance: none;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--surface);
    color: #f1ece2;
    cursor: pointer;
    padding: 0.63rem 2.35rem 0.63rem 0.85rem;
    transition: border-color 180ms var(--ease), background 180ms var(--ease);
  }

  select:hover:not(:disabled) {
    border-color: var(--line-strong);
    background: var(--surface-hover);
  }

  .scan-button {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 9px;
    min-width: 8.25rem;
    min-height: 38px;
    overflow: hidden;
    border: 0;
    border-radius: 8px;
    background: linear-gradient(180deg, #efffb6, var(--accent));
    color: var(--accent-ink);
    cursor: pointer;
    font-weight: 820;
    padding: 0 1rem;
    transition: transform 180ms var(--ease), box-shadow 180ms var(--ease), filter 180ms var(--ease);
  }

  .scan-button:hover:not(:disabled) {
    box-shadow: 0 12px 30px rgba(223, 245, 154, 0.23);
    filter: saturate(1.04);
    transform: translateY(-1px);
  }

  .scan-button:active:not(:disabled) {
    transform: translateY(0);
  }

  .button-icon {
    width: 14px;
    height: 14px;
    border: 2px solid currentColor;
    border-left-color: transparent;
    border-radius: 999px;
  }

  .button-icon.active {
    animation: spin 850ms linear infinite;
  }

  .scan-button:disabled,
  select:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }

  .status-banner {
    position: relative;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 18px;
    min-height: 76px;
    overflow: hidden;
    border-bottom: 1px solid var(--line);
    background:
      linear-gradient(90deg, rgba(223, 245, 154, 0.07), transparent 54%),
      rgba(14, 18, 16, 0.9);
    padding: 13px 24px;
    box-sizing: border-box;
  }

  .status-left {
    display: flex;
    align-items: center;
    gap: 14px;
    min-width: 0;
  }

  .status-dot {
    width: 9px;
    height: 9px;
    flex: 0 0 auto;
    border-radius: 999px;
    background: var(--accent);
    box-shadow: 0 0 12px rgba(223, 245, 154, 0.44);
  }

  .status-dot.active {
    animation: pulse 1.45s var(--ease) infinite;
  }

  .status-copy {
    display: grid;
    gap: 3px;
    min-width: 0;
  }

  .status-copy strong {
    overflow: hidden;
    color: #fbf6eb;
    font-size: 0.93rem;
    font-weight: 760;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .profile-line {
    overflow: hidden;
    max-width: 72vw;
    color: var(--text-dim) !important;
    font-variant-numeric: tabular-nums;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .metrics {
    display: grid;
    grid-template-columns: repeat(4, minmax(5.75rem, auto));
    gap: 28px;
  }

  .metrics div {
    display: grid;
    gap: 2px;
    min-width: 0;
    text-align: right;
  }

  .metrics span {
    font-weight: 800;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .metrics strong {
    overflow: hidden;
    color: #fbf6eb;
    font-size: 0.96rem;
    font-variant-numeric: tabular-nums;
    font-weight: 760;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .metrics div:nth-child(3) strong {
    color: var(--accent);
  }

  .activity-line {
    position: absolute;
    right: 24px;
    bottom: 0;
    left: 24px;
    height: 2px;
    overflow: hidden;
    background: rgba(223, 245, 154, 0.07);
  }

  .activity-line span {
    display: block;
    width: 34%;
    height: 100%;
    background: linear-gradient(90deg, transparent, var(--accent), var(--accent-2), transparent);
    transform: translateX(-100%);
  }

  .activity-line.active span {
    animation: scan-line 1.35s var(--ease) infinite;
  }

  .workspace {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(21rem, 24rem);
    min-height: 0;
    overflow: hidden;
  }

  .visual-stage,
  .analysis-rail {
    min-height: 0;
    background: rgba(13, 17, 14, 0.72);
  }

  .visual-stage {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .analysis-rail {
    display: grid;
    grid-template-rows: minmax(14rem, 1fr) minmax(14rem, 1fr);
    overflow: hidden;
    border-left: 1px solid var(--line);
    background: rgba(21, 23, 18, 0.94);
  }

  .navigator,
  .inspector {
    min-height: 0;
    overflow: hidden;
  }

  .inspector {
    border-top: 1px solid var(--line);
    padding: 16px 20px;
    box-sizing: border-box;
  }

  .stage-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 62px;
    padding: 12px 24px 10px;
    box-sizing: border-box;
  }

  .stage-context {
    display: grid;
    gap: 5px;
    min-width: 0;
  }

  .stage-context > span {
    color: var(--text-muted);
    font-size: 0.72rem;
    font-weight: 850;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .breadcrumb {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    min-width: 0;
  }

  .bc-item,
  .view-tabs button {
    border: none;
    background: transparent;
    color: #e9e1d5;
    cursor: pointer;
  }

  .bc-item {
    max-width: 12rem;
    overflow: hidden;
    padding: 0.2rem 0.35rem;
    border-radius: 6px;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: color 160ms var(--ease), background 160ms var(--ease);
  }

  .bc-item:hover {
    background: rgba(223, 245, 154, 0.08);
    color: var(--accent);
  }

  .sep {
    color: #6d746d;
  }

  .view-tabs {
    display: flex;
    align-items: center;
    gap: 4px;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--surface);
    flex-shrink: 0;
    padding: 4px;
  }

  .view-tabs button {
    min-height: 30px;
    border-radius: 6px;
    color: var(--text-muted);
    padding: 0 0.85rem;
    transition: color 160ms var(--ease), background 160ms var(--ease);
  }

  .view-tabs button:hover {
    color: #f6f0e5;
  }

  .view-tabs button.active {
    background: var(--surface-active);
    color: #fbf6eb;
    font-weight: 800;
  }

  .visual-content {
    flex: 1;
    min-height: 0;
    display: flex;
    overflow: hidden;
    padding: 0 24px 24px;
  }

  .visual-content > :global(*) {
    min-height: 0;
  }

  .empty-state {
    display: grid;
    place-content: center;
    min-height: 0;
    border-top: 1px solid var(--line);
    background: rgba(21, 23, 18, 0.72);
    color: var(--text-muted);
    padding: 2rem;
    text-align: center;
  }

  .start-board {
    display: grid;
    grid-template-columns: minmax(18rem, 0.82fr) minmax(20rem, 1fr);
    min-height: 0;
    overflow: hidden;
    border-top: 1px solid var(--line);
    background: rgba(13, 17, 14, 0.72);
  }

  .start-copy,
  .drive-board {
    min-height: 0;
  }

  .start-copy {
    display: grid;
    align-content: center;
    gap: 24px;
    border-right: 1px solid var(--line);
    background:
      linear-gradient(135deg, rgba(223, 245, 154, 0.08), transparent 48%),
      rgba(21, 23, 18, 0.68);
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
    max-width: 34rem;
  }

  .start-copy strong {
    color: #fbf6eb;
    font-size: clamp(1.6rem, 3.1vw, 3.1rem);
    font-weight: 820;
    line-height: 1.02;
  }

  .start-copy span {
    color: var(--text-muted);
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
    background: var(--surface);
    color: #ebe4d8;
    cursor: pointer;
    padding: 0.85rem 1rem;
    text-align: left;
    transition: border-color 180ms var(--ease), background 180ms var(--ease), transform 180ms var(--ease);
  }

  .drive-board button:hover:not(:disabled),
  .drive-board button.selected {
    border-color: rgba(223, 245, 154, 0.36);
    background: var(--surface-hover);
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
    color: #fbf6eb;
    font-size: 1.08rem;
  }

  .drive-board small,
  .drive-board em {
    overflow: hidden;
    color: var(--text-muted);
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

  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
      box-shadow: 0 0 0 5px rgba(223, 245, 154, 0.08), 0 0 14px rgba(223, 245, 154, 0.34);
    }
    50% {
      opacity: 0.62;
      box-shadow: 0 0 0 9px rgba(223, 245, 154, 0.13), 0 0 22px rgba(223, 245, 154, 0.22);
    }
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes scan-line {
    to {
      transform: translateX(300%);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    *,
    *::before,
    *::after {
      animation-duration: 0.01ms !important;
      animation-iteration-count: 1 !important;
      scroll-behavior: auto !important;
      transition-duration: 0.01ms !important;
    }
  }

  @media (max-width: 1180px) {
    .workspace {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: minmax(32rem, 1fr) minmax(18rem, 0.58fr);
    }

    .analysis-rail {
      grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
      grid-template-rows: minmax(0, 1fr);
      border-top: 1px solid var(--line);
      border-left: none;
    }

    .inspector {
      border-top: none;
      border-left: 1px solid var(--line);
    }

    .metrics {
      gap: 18px;
    }
  }

  @media (max-width: 860px) {
    :global(body) {
      overflow: auto;
    }

    .shell {
      height: auto;
      min-height: 100vh;
      overflow: visible;
      grid-template-rows: auto auto minmax(34rem, 1fr);
    }

    .top-bar,
    .top-controls,
    .status-banner,
    .status-left {
      align-items: stretch;
    }

    .top-bar,
    .top-controls {
      flex-direction: column;
    }

    .top-bar {
      padding: 14px;
    }

    .brand {
      width: 100%;
    }

    .top-controls {
      width: 100%;
      min-width: 0;
      gap: 10px;
    }

    .drive-picker {
      grid-template-columns: 1fr;
      width: 100%;
    }

    .scan-button {
      width: 100%;
    }

    .status-banner {
      grid-template-columns: 1fr;
      padding: 14px;
    }

    .metrics {
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 12px;
    }

    .metrics div {
      text-align: left;
    }

    .workspace {
      grid-template-columns: 1fr;
      grid-template-rows: minmax(30rem, 1fr) minmax(34rem, 0.9fr);
    }

    .analysis-rail,
    .start-board {
      grid-template-columns: 1fr;
      grid-template-rows: auto auto;
    }

    .analysis-rail {
      min-height: 34rem;
    }

    .inspector {
      border-left: none;
      border-top: 1px solid var(--line);
    }

    .start-copy {
      border-right: none;
      border-bottom: 1px solid var(--line);
    }
  }

  @media (max-width: 560px) {
    .brand-mark {
      width: 38px;
      height: 38px;
    }

    .status-left {
      flex-direction: row;
    }

    .metrics {
      grid-template-columns: 1fr;
    }

    .stage-toolbar {
      align-items: stretch;
      flex-direction: column;
      padding: 12px 14px;
    }

    .visual-content {
      padding: 0 14px 14px;
    }

    .view-tabs {
      width: 100%;
    }

    .view-tabs button {
      flex: 1;
      padding-inline: 0.45rem;
    }

    .start-logo {
      width: 100%;
    }
  }
</style>
