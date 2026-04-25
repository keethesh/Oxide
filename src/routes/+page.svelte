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
      console.info("[oxide] scan profile", result.timings);
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

  function formatDuration(durationMs: number): string {
    if (durationMs < 1000) {
      return `${durationMs} ms`;
    }

    const seconds = durationMs / 1000;
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
  const visibleDuration = $derived(scanResult?.duration_ms ?? progress.duration_ms);
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
</script>

<svelte:head>
  <title>Oxide</title>
</svelte:head>

<main class="shell">
  <header class="topbar">
    <div class="brand">
      <strong>Oxide</strong>
      <span>NTFS disk analyzer</span>
    </div>

    <div class="scan-strip">
      <label class="drive-picker">
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
        {isScanning ? "Scanning" : "Scan"}
      </button>
    </div>
  </header>

  <section class="status-rail">
    <div class="status-copy">
      <span class="status-kicker">{progress.phase}</span>
      <strong>{status}</strong>
      <span>{fallbackSummary || scanModeSummary}</span>
      {#if profileSummary}
        <span class="profile-line">{profileSummary}</span>
      {/if}
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
        <strong>{formatDuration(visibleDuration)}</strong>
      </div>
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
      <aside class="navigator" aria-label="Folder navigator">
        <TreeView
          scanLoaded={true}
          scanRootId={scanResult.root_id}
          selectedId={rootId}
          onSelect={handleNavigate}
        />
      </aside>

      <section class="visual-stage" aria-label="Disk visualization">
        <div class="stage-toolbar">
          <div class="breadcrumb">
            {#each breadcrumbPath as [id, name], index (id)}
              {#if index > 0}
                <span class="sep">/</span>
              {/if}
              <button class="bc-item" onclick={() => (rootId = id)}>{name}</button>
            {/each}
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

      <aside class="inspector" aria-label="Largest files">
        <FileList scanLoaded={true} {rootId} onNavigate={handleNavigate} />
      </aside>
    </div>
  {:else}
    <section class="empty-state start">
      <strong>Choose a drive to inspect.</strong>
      <span>Oxide will map the folder tree, render a treemap, and keep the largest files close at hand.</span>
    </section>
  {/if}
</main>

<style>
  :global(body) {
    margin: 0;
    padding: 0;
    min-height: 100vh;
    background: #111312;
    color: #ece8df;
    font-family: "Aptos", "Segoe UI", sans-serif;
  }

  :global(button),
  :global(select) {
    font: inherit;
  }

  .shell {
    --panel: #181b19;
    --panel-strong: #20231f;
    --line: #313730;
    --line-soft: rgba(236, 232, 223, 0.09);
    --text-muted: #a8a094;
    --accent: #d7ff6f;
    --accent-ink: #1c220b;
    --warn: #ffb199;
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    gap: 10px;
    height: 100vh;
    padding: 10px;
    box-sizing: border-box;
  }

  .topbar,
  .status-rail,
  .workspace {
    border: 1px solid var(--line);
    background: var(--panel);
  }

  .topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    min-height: 58px;
    padding: 8px 10px 8px 14px;
    box-sizing: border-box;
  }

  .brand {
    display: flex;
    align-items: baseline;
    gap: 10px;
    min-width: 10rem;
  }

  .brand strong {
    color: #f6f2e9;
    font-size: 1.15rem;
    letter-spacing: 0;
  }

  .brand span,
  .drive-picker span,
  .status-copy span,
  .metrics span {
    color: var(--text-muted);
    font-size: 0.78rem;
  }

  .scan-strip {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    min-width: min(42rem, 60vw);
  }

  .drive-picker {
    display: grid;
    grid-template-columns: auto minmax(14rem, 26rem);
    align-items: center;
    gap: 8px;
    min-width: 0;
    flex: 1;
  }

  select {
    min-width: 0;
    width: 100%;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: #111312;
    color: #ece8df;
    padding: 0.55rem 0.65rem;
  }

  .scan-button {
    min-width: 5.75rem;
    border: 1px solid color-mix(in srgb, var(--accent), #111312 35%);
    border-radius: 6px;
    background: var(--accent);
    color: var(--accent-ink);
    cursor: pointer;
    font-weight: 800;
    padding: 0.58rem 0.9rem;
  }

  .scan-button:disabled,
  select:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }

  .status-rail {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    min-height: 74px;
    padding: 10px 14px;
    box-sizing: border-box;
  }

  .status-copy {
    display: grid;
    gap: 3px;
    min-width: 0;
  }

  .status-copy strong {
    overflow: hidden;
    color: #f6f2e9;
    font-size: 1rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .profile-line {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .status-kicker {
    color: var(--accent) !important;
    text-transform: uppercase;
  }

  .metrics {
    display: grid;
    grid-template-columns: repeat(4, minmax(6.5rem, auto));
    gap: 1px;
    overflow: hidden;
    border: 1px solid var(--line-soft);
    background: var(--line-soft);
  }

  .metrics div {
    display: grid;
    gap: 2px;
    min-width: 0;
    background: #151715;
    padding: 8px 10px;
  }

  .metrics strong {
    overflow: hidden;
    color: #f6f2e9;
    font-size: 0.95rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .workspace {
    display: grid;
    grid-template-columns: minmax(16rem, 22rem) minmax(0, 1fr) minmax(20rem, 28rem);
    gap: 1px;
    min-height: 0;
    overflow: hidden;
    background: var(--line);
  }

  .navigator,
  .visual-stage,
  .inspector {
    background: var(--panel);
    min-height: 0;
  }

  .navigator,
  .inspector {
    overflow: auto;
  }

  .visual-stage {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .stage-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    min-height: 48px;
    padding: 7px 8px 7px 12px;
    border-bottom: 1px solid var(--line);
    box-sizing: border-box;
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
    color: #e8e1d6;
    cursor: pointer;
  }

  .bc-item {
    max-width: 12rem;
    overflow: hidden;
    padding: 0.25rem 0.35rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .bc-item:hover {
    color: var(--accent);
  }

  .sep {
    color: #6d746d;
  }

  .view-tabs {
    display: flex;
    align-items: center;
    gap: 1px;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--line);
    flex-shrink: 0;
  }

  .view-tabs button {
    background: #151715;
    color: var(--text-muted);
    padding: 0.42rem 0.7rem;
  }

  .view-tabs button.active {
    background: #e7edda;
    color: #151715;
    font-weight: 800;
  }

  .visual-content {
    flex: 1;
    min-height: 0;
    display: flex;
    padding: 8px;
    overflow: hidden;
  }

  .visual-content > :global(*) {
    min-height: 0;
  }

  .empty-state {
    display: grid;
    place-content: center;
    gap: 8px;
    min-height: 0;
    border: 1px dashed var(--line);
    background: var(--panel);
    color: var(--text-muted);
    padding: 2rem;
    text-align: center;
  }

  .empty-state strong {
    color: #f6f2e9;
  }

  @media (max-width: 1180px) {
    .workspace {
      grid-template-columns: minmax(15rem, 20rem) minmax(0, 1fr);
    }

    .inspector {
      display: none;
    }
  }

  @media (max-width: 860px) {
    .shell {
      height: auto;
      min-height: 100vh;
      grid-template-rows: auto auto minmax(34rem, 1fr);
    }

    .topbar,
    .status-rail {
      grid-template-columns: 1fr;
    }

    .topbar,
    .scan-strip,
    .status-rail {
      align-items: stretch;
    }

    .topbar,
    .scan-strip {
      flex-direction: column;
    }

    .drive-picker {
      grid-template-columns: 1fr;
    }

    .metrics {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .workspace {
      grid-template-columns: 1fr;
      grid-template-rows: minmax(12rem, 16rem) minmax(28rem, 1fr);
    }
  }

  @media (max-width: 520px) {
    .shell {
      padding: 6px;
      gap: 6px;
    }

    .brand {
      align-items: flex-start;
      flex-direction: column;
      gap: 2px;
    }

    .metrics {
      grid-template-columns: 1fr;
    }

    .stage-toolbar {
      align-items: stretch;
      flex-direction: column;
    }

    .view-tabs {
      width: 100%;
    }

    .view-tabs button {
      flex: 1;
    }
  }
</style>
