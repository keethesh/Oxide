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
  <header class="topbar">
    <div class="brand">
      <span class="brand-mark" aria-hidden="true"></span>
      <span class="brand-copy">
        <strong>Oxide</strong>
        <span>NTFS disk analyzer</span>
      </span>
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
        <span>{isScanning ? "Scanning" : "Scan drive"}</span>
      </button>
    </div>
  </header>

  <section class="status-rail">
    <div class:active={isScanning} class="scan-orb" aria-hidden="true">
      <span></span>
    </div>

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
            <span>{activeTab === "treemap" ? "Space map" : "Largest files"}</span>
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
        <span class="empty-mark" aria-hidden="true"></span>
        <div>
          <strong>Choose a drive to inspect.</strong>
          <span>Oxide opens with a map-first workspace so storage pressure is visible before you start drilling into folders.</span>
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
    padding: 0;
    min-height: 100vh;
    background:
      radial-gradient(circle at 18% 8%, rgba(159, 175, 119, 0.22), transparent 24rem),
      linear-gradient(135deg, #151712 0%, #0e1210 44%, #15110f 100%);
    color: #f1ece2;
    font-family: "Segoe UI Variable", "Aptos", "Segoe UI", sans-serif;
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

  .shell {
    --panel: oklch(20% 0.018 125);
    --panel-raised: oklch(24% 0.019 125);
    --panel-strong: oklch(27% 0.02 115);
    --line: rgba(238, 232, 219, 0.13);
    --line-strong: rgba(238, 232, 219, 0.22);
    --line-soft: rgba(238, 232, 219, 0.075);
    --text-muted: #aaa397;
    --text-soft: #d6cfc1;
    --accent: #dff59a;
    --accent-2: #f2b16f;
    --accent-ink: #20240c;
    --warn: #ffb299;
    --ease: cubic-bezier(0.16, 1, 0.3, 1);
    display: grid;
    grid-template-rows: auto auto minmax(0, 1fr);
    gap: 12px;
    height: 100vh;
    padding: 12px;
    box-sizing: border-box;
  }

  .topbar,
  .status-rail,
  .workspace {
    border: 1px solid var(--line);
    box-shadow: 0 20px 70px rgba(0, 0, 0, 0.22);
  }

  .topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    min-height: 64px;
    padding: 10px 12px 10px 14px;
    border-radius: 10px;
    background:
      linear-gradient(180deg, rgba(255, 252, 239, 0.055), transparent),
      var(--panel);
    box-sizing: border-box;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 10rem;
  }

  .brand-mark {
    width: 28px;
    height: 28px;
    border: 1px solid rgba(223, 245, 154, 0.5);
    border-radius: 8px;
    background:
      linear-gradient(135deg, rgba(223, 245, 154, 0.95), rgba(242, 177, 111, 0.85)),
      #dff59a;
    box-shadow: inset 0 0 0 6px rgba(21, 23, 18, 0.18), 0 10px 25px rgba(223, 245, 154, 0.18);
  }

  .brand-copy {
    display: grid;
    gap: 1px;
  }

  .brand strong {
    color: #fbf6eb;
    font-size: 1.08rem;
    letter-spacing: 0;
    line-height: 1;
  }

  .brand-copy span,
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
    gap: 10px;
    min-width: min(42rem, 60vw);
  }

  .drive-picker {
    position: relative;
    display: grid;
    grid-template-columns: auto minmax(14rem, 27rem);
    align-items: center;
    gap: 10px;
    min-width: 0;
    flex: 1;
  }

  select {
    min-width: 0;
    width: 100%;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: #111511;
    color: #f1ece2;
    padding: 0.66rem 0.75rem;
    transition: border-color 180ms var(--ease), background 180ms var(--ease), transform 180ms var(--ease);
  }

  select:hover:not(:disabled) {
    border-color: var(--line-strong);
    background: #151a14;
  }

  .scan-button {
    position: relative;
    min-width: 7.25rem;
    overflow: hidden;
    border: 1px solid color-mix(in oklch, var(--accent), #111312 35%);
    border-radius: 8px;
    background: linear-gradient(180deg, #efffb6, var(--accent));
    color: var(--accent-ink);
    cursor: pointer;
    font-weight: 800;
    padding: 0.69rem 1rem;
    transition: transform 180ms var(--ease), box-shadow 180ms var(--ease), filter 180ms var(--ease);
  }

  .scan-button::after {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(110deg, transparent, rgba(255, 255, 255, 0.36), transparent);
    transform: translateX(-120%);
    transition: transform 600ms var(--ease);
  }

  .scan-button:hover:not(:disabled) {
    box-shadow: 0 12px 34px rgba(223, 245, 154, 0.2);
    filter: saturate(1.04);
    transform: translateY(-1px);
  }

  .scan-button:hover:not(:disabled)::after {
    transform: translateX(120%);
  }

  .scan-button:active:not(:disabled) {
    transform: translateY(0);
  }

  .scan-button:disabled,
  select:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }

  .status-rail {
    position: relative;
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 14px;
    min-height: 84px;
    overflow: hidden;
    border-radius: 12px;
    background:
      linear-gradient(180deg, rgba(255, 252, 239, 0.045), transparent),
      var(--panel-raised);
    padding: 12px 15px;
    box-sizing: border-box;
  }

  .scan-orb {
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    border-radius: 10px;
    background: #151914;
    border: 1px solid var(--line);
  }

  .scan-orb span {
    width: 12px;
    height: 12px;
    border-radius: 999px;
    background: var(--text-muted);
    box-shadow: 0 0 0 6px rgba(238, 232, 219, 0.04);
  }

  .scan-orb.active span {
    background: var(--accent);
    animation: pulse 1.4s var(--ease) infinite;
  }

  .status-copy {
    display: grid;
    gap: 4px;
    min-width: 0;
  }

  .status-copy strong {
    overflow: hidden;
    color: #fbf6eb;
    font-size: clamp(1rem, 1.4vw, 1.22rem);
    font-weight: 720;
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
    font-weight: 800;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .metrics {
    display: grid;
    grid-template-columns: repeat(4, minmax(6.5rem, auto));
    gap: 0;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: #151914;
  }

  .metrics div {
    display: grid;
    gap: 3px;
    min-width: 0;
    padding: 9px 12px;
    border-right: 1px solid var(--line-soft);
  }

  .metrics div:last-child {
    border-right: none;
  }

  .metrics strong {
    overflow: hidden;
    color: #fbf6eb;
    font-size: 1rem;
    font-variant-numeric: tabular-nums;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .activity-line {
    position: absolute;
    right: 14px;
    bottom: 0;
    left: 14px;
    height: 2px;
    overflow: hidden;
    background: rgba(238, 232, 219, 0.08);
  }

  .activity-line span {
    display: block;
    width: 38%;
    height: 100%;
    background: linear-gradient(90deg, transparent, var(--accent), var(--accent-2), transparent);
    transform: translateX(-100%);
  }

  .activity-line.active span {
    animation: scan-line 1.45s var(--ease) infinite;
  }

  .workspace {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(23rem, 30rem);
    gap: 1px;
    min-height: 0;
    overflow: hidden;
    border-radius: 12px;
    background: var(--line);
  }

  .navigator,
  .visual-stage,
  .inspector,
  .analysis-rail {
    background: var(--panel);
    min-height: 0;
  }

  .analysis-rail {
    display: grid;
    grid-template-rows: minmax(15rem, 1.08fr) minmax(15rem, 0.92fr);
    gap: 1px;
    overflow: hidden;
    background: var(--line);
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
    min-height: 54px;
    padding: 8px 10px 8px 14px;
    border-bottom: 1px solid var(--line);
    box-sizing: border-box;
    background: linear-gradient(180deg, rgba(255, 252, 239, 0.035), transparent);
  }

  .stage-context {
    display: grid;
    gap: 4px;
    min-width: 0;
  }

  .stage-context > span {
    color: var(--accent);
    font-size: 0.68rem;
    font-weight: 800;
    letter-spacing: 0.08em;
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
    padding: 0.25rem 0.35rem;
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
    gap: 2px;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: #121611;
    flex-shrink: 0;
    padding: 2px;
  }

  .view-tabs button {
    border-radius: 6px;
    color: var(--text-muted);
    padding: 0.48rem 0.78rem;
    transition: color 160ms var(--ease), background 160ms var(--ease);
  }

  .view-tabs button:hover {
    color: #f6f0e5;
  }

  .view-tabs button.active {
    background: #e6edd6;
    color: #151715;
    font-weight: 800;
  }

  .visual-content {
    flex: 1;
    min-height: 0;
    display: flex;
    padding: 10px;
    overflow: hidden;
  }

  .visual-content > :global(*) {
    min-height: 0;
  }

  .empty-state {
    display: grid;
    place-content: center;
    gap: 10px;
    min-height: 0;
    border: 1px dashed var(--line-strong);
    border-radius: 12px;
    background:
      linear-gradient(180deg, rgba(255, 252, 239, 0.045), transparent),
      var(--panel);
    color: var(--text-muted);
    padding: 2rem;
    text-align: center;
  }

  .start-board {
    display: grid;
    grid-template-columns: minmax(18rem, 0.9fr) minmax(20rem, 1.1fr);
    gap: 1px;
    min-height: 0;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--line);
    box-shadow: 0 20px 70px rgba(0, 0, 0, 0.22);
  }

  .start-copy,
  .drive-board {
    min-height: 0;
    background:
      linear-gradient(180deg, rgba(255, 252, 239, 0.045), transparent),
      var(--panel);
  }

  .start-copy {
    display: grid;
    align-content: center;
    gap: 18px;
    padding: clamp(1.25rem, 4vw, 3rem);
  }

  .start-copy div {
    display: grid;
    gap: 0.5rem;
    max-width: 32rem;
  }

  .start-copy strong {
    color: #fbf6eb;
    font-size: clamp(1.55rem, 3vw, 2.7rem);
    line-height: 1.02;
  }

  .start-copy span:not(.empty-mark) {
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
    min-height: 74px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: #121611;
    color: #ebe4d8;
    cursor: pointer;
    padding: 0.9rem 1rem;
    text-align: left;
    transition: border-color 180ms var(--ease), background 180ms var(--ease), transform 180ms var(--ease);
  }

  .drive-board button:hover:not(:disabled),
  .drive-board button.selected {
    border-color: rgba(223, 245, 154, 0.46);
    background: linear-gradient(90deg, rgba(223, 245, 154, 0.12), rgba(242, 177, 111, 0.04));
    transform: translateY(-1px);
  }

  .drive-board button:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }

  .drive-board button span {
    display: grid;
    gap: 0.2rem;
    min-width: 0;
  }

  .drive-board button strong {
    color: #fbf6eb;
    font-size: 1.18rem;
  }

  .drive-board small,
  .drive-board em {
    overflow: hidden;
    color: var(--text-muted);
    font-size: 0.8rem;
    font-style: normal;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .drive-board em {
    flex-shrink: 0;
    border: 1px solid var(--line);
    border-radius: 999px;
    padding: 0.25rem 0.55rem;
  }

  .drive-board button.selected em {
    border-color: rgba(223, 245, 154, 0.35);
    color: var(--accent);
  }

  .empty-mark {
    width: 62px;
    height: 62px;
    justify-self: center;
    border-radius: 16px;
    background:
      linear-gradient(90deg, transparent 48%, rgba(21, 23, 18, 0.24) 48% 52%, transparent 52%),
      linear-gradient(0deg, transparent 48%, rgba(21, 23, 18, 0.24) 48% 52%, transparent 52%),
      linear-gradient(135deg, #dff59a, #f2b16f);
    box-shadow: 0 18px 42px rgba(0, 0, 0, 0.24);
  }

  @keyframes pulse {
    0%,
    100% {
      box-shadow: 0 0 0 6px rgba(223, 245, 154, 0.08);
    }
    50% {
      box-shadow: 0 0 0 11px rgba(223, 245, 154, 0.16);
    }
  }

  @keyframes scan-line {
    to {
      transform: translateX(265%);
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
      grid-template-rows: minmax(34rem, 1fr) minmax(18rem, 0.62fr);
    }

    .analysis-rail {
      grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
      grid-template-rows: minmax(0, 1fr);
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

    .status-rail {
      grid-template-columns: auto minmax(0, 1fr);
    }

    .metrics {
      grid-column: 1 / -1;
    }

    .workspace {
      grid-template-columns: 1fr;
      grid-template-rows: minmax(30rem, 1fr) minmax(30rem, 0.8fr);
    }

    .analysis-rail,
    .start-board {
      grid-template-columns: 1fr;
      grid-template-rows: auto auto;
    }
  }

  @media (max-width: 520px) {
    .shell {
      padding: 6px;
      gap: 6px;
    }

    .brand {
      align-items: flex-start;
      gap: 9px;
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
