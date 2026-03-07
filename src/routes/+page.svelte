<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
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

  async function loadDrives() {
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
    const drive = drives.find((candidate) => candidate.letter === requestedDrive) ?? null;
    if (!drive?.supported || isScanning) {
      return;
    }

    isScanning = true;
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
      rootId = result.root_id;
      status = result.fallback_reason
        ? `Scan complete for ${result.drive_letter} using ${modeLabel(result.scan_mode)} after fallback.`
        : `Scan complete for ${result.drive_letter} using ${modeLabel(result.scan_mode)}.`;
    } catch (error) {
      status = `Scan failed: ${error}`;
    } finally {
      isScanning = false;
    }
  }

  async function updateBreadcrumbs() {
    if (!scanResult) {
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

  onMount(() => {
    const unlisten = listen<ScanProgress>("scan-progress", (event) => {
      progress = event.payload;
    });

    void (async () => {
      await loadDrives();

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
      unlisten.then((fn) => fn());
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

  function formatDuration(durationMs: number): string {
    if (durationMs < 1000) {
      return `${durationMs} ms`;
    }

    const seconds = durationMs / 1000;
    return `${seconds.toFixed(seconds >= 10 ? 0 : 1)} s`;
  }

  const selectedDriveInfo = $derived(
    drives.find((drive) => drive.letter === selectedDrive) ?? null
  );
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
</script>

<svelte:head>
  <title>Oxide</title>
</svelte:head>

<main class="container">
  <header>
    <div class="brand">
      <p class="eyebrow">Windows NTFS Disk Analyzer</p>
      <h1>Oxide</h1>
    </div>

    <div class="controls">
      <label class="selector">
        <span>Drive</span>
        <select bind:value={selectedDrive} disabled={drivesLoading || isScanning || !drives.length}>
          {#each drives as drive}
            <option value={drive.letter}>
              {drive.label} [{drive.filesystem}]
            </option>
          {/each}
        </select>
      </label>

      <button
        class="scan-button"
        disabled={!selectedDriveInfo?.supported || isScanning || drivesLoading}
        onclick={() => scan()}
      >
        {isScanning ? "Scanning..." : "Scan Drive"}
      </button>
    </div>
  </header>

  <section class="hero">
    <div>
      <p class="status-label">Status</p>
      <p class="status-text">{status}</p>
      <p class="scan-meta">{scanModeSummary}</p>
      {#if fallbackSummary}
        <p class="scan-meta warning">{fallbackSummary}</p>
      {/if}
    </div>

    <div class="stats">
      <div>
        <span>Files</span>
        <strong>{progress.files_scanned.toLocaleString()}</strong>
      </div>
      <div>
        <span>Folders</span>
        <strong>{progress.dirs_scanned.toLocaleString()}</strong>
      </div>
      <div>
        <span>Bytes</span>
        <strong>{formatSize(progress.bytes_scanned)}</strong>
      </div>
      <div>
        <span>Elapsed</span>
        <strong>{formatDuration(progress.duration_ms)}</strong>
      </div>
    </div>
  </section>

  {#if drivesLoading}
    <section class="panel empty">Loading drives...</section>
  {:else if !drives.length}
    <section class="panel empty">No local drives were detected on this system.</section>
  {:else if selectedDriveInfo && !selectedDriveInfo.supported}
    <section class="panel empty">
      {selectedDriveInfo.letter} is formatted as {selectedDriveInfo.filesystem}. This MVP only supports NTFS volumes.
    </section>
  {:else if scanResult}
    <div class="app-layout">
      <aside class="sidebar">
        <TreeView
          scanLoaded={true}
          scanRootId={scanResult.root_id}
          selectedId={rootId}
          onSelect={handleNavigate}
        />
      </aside>

      <section class="main-view">
        <div class="toolbar">
          <div class="breadcrumb">
            {#each breadcrumbPath as [id, name], index (id)}
              {#if index > 0}
                <span class="sep">/</span>
              {/if}
              <button class="bc-item" onclick={() => (rootId = id)}>{name}</button>
            {/each}
          </div>

          <nav class="tabs">
            <button class:active={activeTab === "treemap"} onclick={() => (activeTab = "treemap")}>
              Treemap
            </button>
            <button class:active={activeTab === "list"} onclick={() => (activeTab = "list")}>
              Largest Files
            </button>
          </nav>
        </div>

        <div class="content">
          {#if activeTab === "treemap"}
            <Treemap {rootId} onNavigate={handleNavigate} />
          {:else}
            <FileList scanLoaded={true} {rootId} onNavigate={handleNavigate} />
          {/if}
        </div>
      </section>
    </div>
  {:else}
    <section class="panel empty">
      Select an NTFS drive and run a scan. Oxide will build a treemap, folder hierarchy, and largest-file view for the scanned volume.
    </section>
  {/if}

  <footer class="status-bar">
    <div class="progress-info">
      <span class="phase">{progress.phase}</span>
      {#if currentScanMode}
        <span>{modeLabel(currentScanMode)}</span>
      {/if}
      {#if currentFallbackReason}
        <span class="warning">{fallbackDescription(currentFallbackReason)}</span>
      {/if}
      {#if progress.done && scanResult}
        <span>{scanResult.drive_letter} ready</span>
      {/if}
    </div>
    <p class="status-msg">NTFS-only MVP. Oxide now prefers raw MFT scans and falls back automatically when needed.</p>
  </footer>
</main>

<style>
  :global(body) {
    margin: 0;
    padding: 0;
    min-height: 100vh;
    background:
      radial-gradient(circle at top left, rgba(255, 96, 46, 0.2), transparent 28%),
      linear-gradient(180deg, #181818 0%, #111111 100%);
    color: #f1f1f1;
    font-family: "Segoe UI", Tahoma, Geneva, Verdana, sans-serif;
  }

  .container {
    display: flex;
    flex-direction: column;
    min-height: 100vh;
  }

  header {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    padding: 1.5rem 2rem 1rem;
  }

  .brand,
  .controls,
  .stats {
    display: flex;
    gap: 1rem;
  }

  .brand {
    flex-direction: column;
  }

  .eyebrow,
  .status-label {
    margin: 0;
    color: #ff9c79;
    font-size: 0.78rem;
    text-transform: uppercase;
    letter-spacing: 0.12em;
  }

  h1,
  .status-text {
    margin: 0;
  }

  h1 {
    font-size: clamp(2rem, 4vw, 3rem);
    line-height: 1;
  }

  .controls {
    align-items: end;
    flex-wrap: wrap;
  }

  .selector {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    font-size: 0.85rem;
    color: #b9b9b9;
  }

  select,
  .scan-button {
    border-radius: 999px;
    border: 1px solid #393939;
    background: rgba(255, 255, 255, 0.06);
    color: #fff;
    padding: 0.7rem 1rem;
    font: inherit;
  }

  select {
    min-width: 16rem;
  }

  .scan-button {
    background: linear-gradient(135deg, #ff5d2a, #ff8b67);
    border-color: transparent;
    cursor: pointer;
    font-weight: 700;
  }

  .scan-button:disabled,
  select:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }

  .hero {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    padding: 0 2rem 1rem;
  }

  .status-text {
    font-size: 1.05rem;
    color: #f5f5f5;
  }

  .scan-meta {
    margin: 0.4rem 0 0;
    color: #bcbcbc;
    font-size: 0.9rem;
  }

  .warning {
    color: #ffb49b;
  }

  .stats {
    flex-wrap: wrap;
  }

  .stats div {
    min-width: 8rem;
    border: 1px solid #2d2d2d;
    border-radius: 16px;
    background: rgba(255, 255, 255, 0.03);
    padding: 0.85rem 1rem;
  }

  .stats span {
    display: block;
    color: #9e9e9e;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .stats strong {
    display: block;
    margin-top: 0.25rem;
    font-size: 1rem;
  }

  .panel,
  .app-layout {
    flex: 1;
    min-height: 0;
  }

  .panel.empty {
    margin: 0 2rem 1.5rem;
    border: 1px dashed #3b3b3b;
    border-radius: 24px;
    padding: 1.5rem;
    color: #b2b2b2;
    background: rgba(255, 255, 255, 0.02);
  }

  .app-layout {
    display: flex;
    gap: 1rem;
    padding: 0 2rem 1.5rem;
  }

  .sidebar,
  .main-view {
    border: 1px solid #2d2d2d;
    border-radius: 24px;
    background: rgba(12, 12, 12, 0.88);
    min-height: 0;
  }

  .sidebar {
    width: 21rem;
    overflow: auto;
  }

  .main-view {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .toolbar {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    padding: 1rem 1rem 0;
    align-items: center;
    flex-wrap: wrap;
  }

  .breadcrumb {
    display: flex;
    gap: 0.4rem;
    align-items: center;
    flex-wrap: wrap;
  }

  .bc-item,
  .tabs button {
    border: none;
    background: transparent;
    color: #ff8b67;
    cursor: pointer;
    font: inherit;
  }

  .sep {
    color: #666;
  }

  .tabs {
    display: flex;
    gap: 0.5rem;
  }

  .tabs button {
    border-radius: 999px;
    padding: 0.55rem 0.95rem;
    color: #a7a7a7;
  }

  .tabs button.active {
    background: rgba(255, 93, 42, 0.14);
    color: #fff;
  }

  .content {
    flex: 1;
    min-height: 0;
    padding: 1rem;
    overflow: auto;
  }

  .status-bar {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.85rem 2rem 1rem;
    border-top: 1px solid #262626;
    color: #9a9a9a;
    font-size: 0.85rem;
  }

  .progress-info {
    display: flex;
    gap: 0.75rem;
    flex-wrap: wrap;
  }

  .phase {
    color: #ff8b67;
    font-weight: 700;
  }

  .status-msg {
    margin: 0;
    text-align: right;
  }

  @media (max-width: 960px) {
    header,
    .hero,
    .app-layout,
    .status-bar {
      padding-left: 1rem;
      padding-right: 1rem;
    }

    .app-layout {
      flex-direction: column;
    }

    .sidebar {
      width: auto;
      max-height: 18rem;
    }

    .status-bar {
      flex-direction: column;
    }

    .status-msg {
      text-align: left;
    }
  }
</style>
