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

  // ── State ──────────────────────────────────────────────────
  let drives = $state<DriveInfo[]>([]);
  let selectedDrive = $state("");
  let activeTab = $state<"treemap" | "files">("treemap");
  let progress = $state<ScanProgress>(idleProgress());
  let scanResult = $state<ScanResult | null>(null);
  let rootId = $state(0);
  let breadcrumbPath = $state<[number, string][]>([]);
  let isScanning = $state(false);
  let drivesLoading = $state(true);
  let scanStartedAt = $state<number | null>(null);
  let liveDurationMs = $state(0);
  let navHistory = $state<number[]>([]);
  let totalItemsEstimate = $state<number | null>(null);
  let showDebugPanel = $state(false);
  let showSidebar = $state(true);
  let scanError = $state<string | null>(null);
  const tauriAvailable = isTauri();

  // ── Drive loading ──────────────────────────────────────────
  async function loadDrives() {
    if (!tauriAvailable) {
      drives = [];
      selectedDrive = "";
      drivesLoading = false;
      return;
    }

    drivesLoading = true;
    try {
      drives = await invoke<DriveInfo[]>("list_drives");
      selectedDrive =
        drives.find((d) => d.supported)?.letter ?? drives[0]?.letter ?? "";
    } catch (error) {
      console.error("[oxide] Failed to list drives:", error);
      drives = [];
    } finally {
      drivesLoading = false;
    }
  }

  // ── Scanning ───────────────────────────────────────────────
  async function scan(requestedDrive = selectedDrive) {
    if (!tauriAvailable) {
      return;
    }

    const drive = drives.find((c) => c.letter === requestedDrive) ?? null;
    if (!drive?.supported || isScanning) {
      return;
    }

    scanError = null;
    isScanning = true;
    scanStartedAt = Date.now();
    liveDurationMs = 0;

    try {
      cancelScan();
      resetScanState();
      const preparation = await invoke<PrepareScanResult>("prepare_scan", {
        driveLetter: drive.letter
      });

      if (preparation.action === "relaunching") {
        progress = idleProgress("Relaunching as admin…");
        return;
      }

      const mode = preparation.mode;
      if (!mode) {
        throw new Error("No scan mode available");
      }

      totalItemsEstimate = preparation.total_items_estimate ?? null;
      progress = idleProgress(
        mode === "mft" ? "Reading MFT…" : "Walking filesystem…",
        mode,
        preparation.fallback_reason
      );

      const result = await invoke<ScanResult>("scan_drive", {
        driveLetter: drive.letter,
        mode
      });

      scanResult = result;
      liveDurationMs = result.duration_ms;
      rootId = result.root_id;
    } catch (error) {
      const msg = String(error);
      if (msg.toLowerCase().includes("cancel")) {
        scanError = "Scan was cancelled.";
      } else {
        scanError = `Scan failed: ${extractTauriError(error)}`;
      }
      resetScanState();
    } finally {
      isScanning = false;
      scanStartedAt = null;
    }
  }

  async function cancelScan() {
    if (!tauriAvailable || !isScanning) return;
    try {
      await invoke("cancel_scan");
    } catch {
      /* already cleaning up */
    }
  }

  // ── Navigation ─────────────────────────────────────────────
  function handleNavigate(id: number) {
    navHistory = [...navHistory, rootId];
    rootId = id;
  }

  function goBack() {
    if (navHistory.length === 0) return;
    rootId = navHistory[navHistory.length - 1];
    navHistory = navHistory.slice(0, -1);
  }
  const canGoBack = $derived(navHistory.length > 0);

  function goToBreadcrumb(index: number) {
    const [id] = breadcrumbPath[index];
    navHistory = breadcrumbPath.slice(index + 1).map(([bid]) => bid);
    rootId = id;
  }

  async function openInExplorer(id: number) {
    try {
      await invoke("open_in_explorer", { nodeId: id });
    } catch (error) {
      console.error("[oxide] Failed to open in explorer:", error);
    }
  }

  // ── Breadcrumb sync ────────────────────────────────────────
  $effect(() => {
    if (scanResult) {
      loadBreadcrumbs();
    }
  });

  async function loadBreadcrumbs() {
    if (!tauriAvailable || !scanResult) return;
    try {
      breadcrumbPath = await invoke<[number, string][]>("get_file_path", {
        id: rootId
      });
    } catch (err) {
      console.error("[oxide] Breadcrumb load failed:", err);
    }
  }

  // ── Timer ──────────────────────────────────────────────────
  $effect(() => {
    if (!isScanning || scanStartedAt === null) return;
    const started = scanStartedAt;
    let frame = 0;
    const tick = () => {
      liveDurationMs = Math.max(liveDurationMs, Date.now() - started);
      frame = window.requestAnimationFrame(tick);
    };
    tick();
    return () => window.cancelAnimationFrame(frame);
  });

  // ── Export snapshot ────────────────────────────────────────
  async function exportSnapshot() {
    const lines = [
      `Oxide — Storage Scan`,
      `Drive:      ${driveLabel}`,
      `Status:     ${scanStatusText}`,
      `Files:      ${activeFiles.toLocaleString()}`,
      `Folders:    ${activeDirs.toLocaleString()}`,
      `Size:       ${formatSize(activeBytes)}`,
      `Duration:   ${durationLabel}`,
      ...(scanResult
        ? [
            `Mode:       ${modeLabel(scanResult.scan_mode)}`,
            `Profile:    scan ${formatDuration(scanResult.timings.scan_ms)} · aggregate ${formatDuration(scanResult.timings.aggregate_ms)} · index ${formatDuration(scanResult.timings.largest_files_ms)} · store ${formatDuration(scanResult.timings.store_ms)} · total ${formatDuration(scanResult.timings.total_ms)}`
          ]
        : [])
    ];

    try {
      await navigator.clipboard.writeText(lines.join("\n"));
    } catch {
      /* clipboard unavailable */
    }
  }

  // ── Reset ──────────────────────────────────────────────────
  function resetScanState() {
    scanResult = null;
    rootId = 0;
    breadcrumbPath = [];
    activeTab = "treemap";
    navHistory = [];
    totalItemsEstimate = null;
    scanError = null;
  }

  function resetApp() {
    cancelScan();
    resetScanState();
    isScanning = false;
    scanStartedAt = null;
    progress = idleProgress();
  }

  // ── Lifecycle ──────────────────────────────────────────────
  onMount(() => {
    if (tauriAvailable) {
      listen<ScanProgress>("scan-progress", (ev) => {
        progress = ev.payload;
      });
    }

    (async () => {
      await loadDrives();
      if (!tauriAvailable) return;
      try {
        const req = await invoke<LaunchScanRequest>("get_launch_scan_request");
        if (req.drive_letter) {
          selectedDrive = req.drive_letter;
          await scan(req.drive_letter);
        }
      } catch (err) {
        console.error("[oxide] Launch request error:", err);
      }
    })();
  });

  // ── Helpers ────────────────────────────────────────────────
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

  function formatSize(bytes: number): string {
    if (bytes === 0) return "0 B";
    const units = ["B", "KB", "MB", "GB", "TB"];
    let v = bytes;
    let i = 0;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i += 1;
    }
    return `${v.toFixed(v >= 10 || i === 0 ? 0 : 1)} ${units[i]}`;
  }

  function formatDuration(ms: number): string {
    if (ms < 1000) return `${ms} ms`;
    const s = ms / 1000;
    return s >= 10 ? `${s.toFixed(0)} s` : `${s.toFixed(1)} s`;
  }

  function modeLabel(mode: ScanMode): string {
    return mode === "mft" ? "MFT" : "Filesystem";
  }

  function extractTauriError(err: unknown): string {
    const msg = String(err);
    return msg
      .replace(/^TAURI: /i, "")
      .replace(/Error:\s*/, "")
      .trim();
  }

  // ── Derived ────────────────────────────────────────────────
  const selectedDriveInfo = $derived(
    drives.find((d) => d.letter === selectedDrive) ?? null
  );

  const visibleDuration = $derived(
    isScanning
      ? Math.max(progress.duration_ms, liveDurationMs)
      : scanResult?.duration_ms ?? progress.duration_ms ?? 0
  );
  const durationLabel = $derived(
    visibleDuration < 1000
      ? `${visibleDuration} ms`
      : `${(visibleDuration / 1000).toFixed(visibleDuration >= 10000 ? 0 : 1)} s`
  );

  const activeFiles = $derived(scanResult?.files_scanned ?? progress.files_scanned);
  const activeDirs = $derived(scanResult?.dirs_scanned ?? progress.dirs_scanned);
  const activeBytes = $derived(scanResult?.bytes_scanned ?? progress.bytes_scanned);

  const scanProgressPercent = $derived.by(() => {
    if (scanResult) return 100;
    if (!isScanning) return 0;
    if (totalItemsEstimate && totalItemsEstimate > 0) {
      const r = (progress.dirs_scanned + progress.files_scanned) / totalItemsEstimate;
      return Math.min(95, (1 - (1 - Math.min(1, r)) ** 2) * 95);
    }
    const elapsedS = liveDurationMs / 1000;
    const expected = progress.scan_mode === "filesystem" ? 55 : 24;
    const r = Math.min(1, elapsedS / expected);
    return Math.min(95, Math.max(4, (1 - (1 - r) ** 2.4) * 95));
  });

  const driveLabel = $derived(
    selectedDriveInfo
      ? `${selectedDriveInfo.letter} · ${selectedDriveInfo.label}`
      : selectedDrive
  );

  const scanStatusText = $derived(
    isScanning
      ? `${progress.phase} · ${durationLabel}`
      : scanResult
        ? `Done · ${durationLabel}`
        : "Ready"
  );

  const scanPercentLabel = $derived(
    scanProgressPercent >= 100 ? "Complete" : `${scanProgressPercent.toFixed(1)}%`
  );

  const scanModeText = $derived(
    scanResult?.scan_mode ? modeLabel(scanResult.scan_mode) : progress.scan_mode ? modeLabel(progress.scan_mode) : "—"
  );

  const entriesLabel = $derived(
    isScanning
      ? `${(activeFiles + activeDirs).toLocaleString()} entries indexed`
      : scanResult
        ? `${(scanResult.files_scanned + scanResult.dirs_scanned).toLocaleString()} entries mapped`
        : "Ready"
  );

  const showStartScreen = $derived(
    !isScanning && !scanResult && !scanError
  );
</script>

<svelte:head>
  <title>Oxide</title>
</svelte:head>

<div class="app">
  <header class="top-bar">
    <div class="top-bar-left">
      <button class="logo-btn" onclick={resetApp} title="Oxide Home" aria-label="Oxide Home">
        <img src="/logo.png" alt="Oxide" class="logo-img" />
      </button>
      {#if !showStartScreen}
        <div class="top-bar-context">
          <span class="drive-label">{driveLabel}</span>
          <span class="scan-status">
            <span class="status-dot" class:active={isScanning} aria-hidden="true"></span>
            {scanStatusText}
          </span>
        </div>
      {/if}
    </div>

    <div class="top-bar-right">
      {#if !isScanning}
        <label class="drive-select-label">
          <span class="sr-only">Select drive</span>
          <div class="select-wrap">
            <select
              bind:value={selectedDrive}
              disabled={drivesLoading || isScanning}
              aria-label="Select drive"
            >
              {#each drives as drive}
                <option
                  value={drive.letter}
                  disabled={!drive.supported}
                >
                  {drive.letter} · {drive.label}
                </option>
              {/each}
              {#if drivesLoading}
                <option disabled>Detecting…</option>
              {/if}
            </select>
          </div>
        </label>
      {/if}
      {#if isScanning || scanResult}
        <div class="stats-row" aria-label="Scan statistics">
          <span class="stat"><strong>{activeFiles.toLocaleString()}</strong> files</span>
          <span class="stat"><strong>{formatSize(activeBytes)}</strong></span>
        </div>
      {/if}
      <button class="icon-btn" class:active={showDebugPanel} onclick={() => (showDebugPanel = !showDebugPanel)} title="Debug panel" aria-label="Toggle debug panel" aria-pressed={showDebugPanel}>
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="3"/><path d="M12 1v4M12 19v4M4.22 4.22l2.83 2.83M16.95 16.95l2.83 2.83M1 12h4M19 12h4M4.22 19.78l2.83-2.83M16.95 7.05l2.83-2.83"/></svg>
      </button>

      <button class="icon-btn" class:active={showSidebar} onclick={() => (showSidebar = !showSidebar)} title="Toggle sidebar" aria-label="Toggle sidebar" aria-pressed={showSidebar}>
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="3" width="18" height="18" rx="2"/><line x1="15" y1="3" x2="15" y2="21"/></svg>
      </button>

      <button
        class="btn-scan"
        disabled={!selectedDriveInfo?.supported || drivesLoading}
        onclick={() => isScanning ? cancelScan() : scan()}
        aria-label={isScanning ? "Cancel scan" : "Start scan"}
      >
        {#if isScanning}
          <svg class="spin-icon" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round">
            <path d="M20 12a8 8 0 1 1-2.34-5.66"/><path d="M20 4v6h-6"/>
          </svg>
          Cancel
        {:else}
          <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round">
            <path d="M20 12a8 8 0 1 1-2.34-5.66"/><path d="M20 4v6h-6"/>
          </svg>
          Scan
        {/if}
      </button>
    </div>
  </header>
  <main class="main-area">
    <div class="content-area">
      {#if showStartScreen}
        <section class="start-screen" aria-label="Welcome">
          <div class="start-content">
            <h1>Where did the space go?</h1>
            <p>Pick a drive, hit <strong>Scan</strong>, and Oxide maps every file and folder so you can find what's hogging your storage.</p>

            {#if drives.length > 0}
              <div class="drive-grid" role="listbox" aria-label="Available drives">
                {#each drives as drive}
                  <button
                    class="drive-card"
                    class:selected={selectedDrive === drive.letter}
                    class:disabled={!drive.supported}
                    disabled={!drive.supported}
                    onclick={() => (selectedDrive = drive.letter)}
                    role="option"
                    aria-selected={selectedDrive === drive.letter}
                  >
                    <div class="drive-card-icon">
                      <svg viewBox="0 0 24 24" width="32" height="32" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                        <rect x="2" y="6" width="20" height="12" rx="2"/>
                        <line x1="6" y1="12" x2="10" y2="12"/>
                        <circle cx="17" cy="12" r="1.5" fill="currentColor"/>
                      </svg>
                    </div>
                    <div class="drive-card-info">
                      <span class="drive-letter">{drive.letter}</span>
                      <span class="drive-name">{drive.label}</span>
                    </div>
                    <div class="drive-card-meta">
                      <span class="drive-fstype">{drive.filesystem}{#if !drive.supported}&middot; unsupported{/if}</span>
                      {#if drive.supported && drive.total_bytes > 0}
                        <span class="drive-free">{formatSize(drive.free_bytes)} free</span>
                      {/if}
                    </div>
                  </button>
                {/each}
              </div>
              <button class="btn-scan btn-scan-lg" disabled={!selectedDriveInfo?.supported || drivesLoading} onclick={() => scan()}>
                <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round">
                  <path d="M20 12a8 8 0 1 1-2.34-5.66"/><path d="M20 4v6h-6"/>
                </svg>
                Scan {selectedDrive}
              </button>
            {:else if !drivesLoading && !tauriAvailable}
              <div class="preview-notice">
                <svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>
                <p>Running in browser preview. Launch the Oxide desktop app to scan your drives.</p>
              </div>
            {:else if drivesLoading}
              <p class="loading-text">Detecting drives…</p>
            {:else}
              <p class="no-drives">No drives found on this system.</p>
            {/if}
          </div>
        </section>

      {/if}
      {#if isScanning}
        <section class="scan-screen" aria-label="Scan in progress" aria-live="polite">
          <div class="scan-card">
            <div class="scan-header">
              <span class="status-dot active" aria-hidden="true"></span>
              Scanning {selectedDrive}
            </div>

            <div class="scan-big-text">{entriesLabel}</div>

            <div class="scan-details">
              <span>{progress.phase}</span>
              <span>{formatSize(activeBytes)} scanned</span>
              <span>{durationLabel}</span>
            </div>

            <div class="progress-bar">
              <div class="progress-fill" style="width: {scanProgressPercent}%"></div>
            </div>

            <span class="progress-label">{scanPercentLabel}</span>

            <button class="btn-scan" onclick={cancelScan}>
              Cancel
            </button>
          </div>
        </section>
      {/if}
      {#if scanError && !isScanning}
        <section class="error-screen" aria-label="Scan error" aria-live="assertive">
          <svg viewBox="0 0 24 24" width="48" height="48" fill="none" stroke="#ffb199" stroke-width="1.5" stroke-linecap="round">
            <circle cx="12" cy="12" r="10"/><line x1="15" y1="9" x2="9" y2="15"/><line x1="9" y1="9" x2="15" y2="15"/>
          </svg>
          <h2>Scan failed</h2>
          <p>{scanError}</p>
          <button class="btn-scan" onclick={() => { scanError = null; resetScanState(); }}>
            Dismiss
          </button>
        </section>
      {/if}
      {#if scanResult && !isScanning}
        <section class="results-screen" aria-label="Scan results">
          <div class="results-toolbar">
            <div class="toolbar-left">
              {#if canGoBack}
                <button class="back-btn" onclick={goBack} title="Go back" aria-label="Go back">
                  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M15 18l-6-6 6-6"/></svg>
                </button>
              {/if}

              <nav class="breadcrumb" aria-label="Current path">
                {#each breadcrumbPath as [id, name], i (id)}
                  {#if i > 0}<span class="bc-sep" aria-hidden="true">/</span>{/if}
                  <button class="bc-item" onclick={() => goToBreadcrumb(i)}>{name}</button>
                {/each}
              </nav>
            </div>

            <div class="toolbar-right">
              <div class="tabs" role="tablist" aria-label="View mode">
                <button
                  class="tab"
                  class:active={activeTab === "treemap"}
                  role="tab"
                  aria-selected={activeTab === "treemap"}
                  onclick={() => (activeTab = "treemap")}
                >
                  <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor"><rect x="3" y="3" width="7" height="9" rx="1"/><rect x="14" y="3" width="7" height="5" rx="1"/><rect x="14" y="12" width="7" height="9" rx="1"/><rect x="3" y="16" width="7" height="5" rx="1"/></svg>
                  Treemap
                </button>
                <button
                  class="tab"
                  class:active={activeTab === "files"}
                  role="tab"
                  aria-selected={activeTab === "files"}
                  onclick={() => (activeTab = "files")}
                >
                  <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor"><rect x="3" y="4" width="18" height="2" rx="1"/><rect x="3" y="11" width="18" height="2" rx="1"/><rect x="3" y="18" width="18" height="2" rx="1"/></svg>
                  Files
                </button>
              </div>

              <button class="icon-btn" onclick={exportSnapshot} title="Copy scan snapshot" aria-label="Copy scan snapshot">
                <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/><rect x="8" y="2" width="8" height="4" rx="1"/></svg>
              </button>
            </div>
          </div>
          <div class="results-content">
            {#if activeTab === "treemap"}
              <Treemap {rootId} onNavigate={handleNavigate} />
            {:else}
              <FileList
                scanLoaded={true}
                {rootId}
                onNavigate={handleNavigate}
                onOpenInExplorer={openInExplorer}
              />
            {/if}
          </div>
          <div class="context-hint">
            Click any block to drill in · Right‑click to open in Explorer · Hover for details
          </div>
        </section>
      {/if}
    </div>
    {#if showSidebar && scanResult && !isScanning}
      <aside class="sidebar" aria-label="Folder sidebar">
        <TreeView
          scanLoaded={true}
          scanRootId={scanResult.root_id}
          selectedId={rootId}
          onSelect={handleNavigate}
        />
      </aside>
    {/if}
  </main>
  <footer class="bottom-bar">
    <span class="bottom-stat">
      <span class="label">Files</span>
      <span class="value">{activeFiles.toLocaleString()}</span>
    </span>
    <span class="separator" aria-hidden="true"></span>
    <span class="bottom-stat">
      <span class="label">Folders</span>
      <span class="value">{activeDirs.toLocaleString()}</span>
    </span>
    <span class="separator" aria-hidden="true"></span>
    <span class="bottom-stat">
      <span class="label">Indexed</span>
      <span class="value">{formatSize(activeBytes)}</span>
    </span>
    <span class="separator" aria-hidden="true"></span>
    <span class="bottom-stat">
      <span class="label">Mode</span>
      <span class="value">{scanModeText}</span>
    </span>
    <span class="separator" aria-hidden="true"></span>
    <span class="bottom-stat">
      <span class="label">Time</span>
      <span class="value">{durationLabel}</span>
    </span>

    {#if showDebugPanel && scanResult}
      <details class="debug-panel" open aria-label="Debug information">
        <summary>Debug</summary>
        <div class="debug-content">
          <span>MFT scan: <strong>{formatDuration(scanResult.timings.scan_ms)}</strong></span>
          <span>Aggregate: <strong>{formatDuration(scanResult.timings.aggregate_ms)}</strong></span>
          <span>Index: <strong>{formatDuration(scanResult.timings.largest_files_ms)}</strong></span>
          <span>Store: <strong>{formatDuration(scanResult.timings.store_ms)}</strong></span>
          <span>Total: <strong>{formatDuration(scanResult.timings.total_ms)}</strong></span>
          <span>Mode: <strong>{scanModeText}</strong></span>
        </div>
      </details>
    {/if}
  </footer>
</div>

<style>
  /* ── Reset & base ──────────────────────────────────────── */
  :global(body) {
    margin: 0;
    height: 100vh;
    overflow: hidden;
    background:
      linear-gradient(90deg, rgba(223, 245, 154, 0.02) 1px, transparent 1px),
      linear-gradient(180deg, rgba(223, 245, 154, 0.015) 1px, transparent 1px),
      radial-gradient(ellipse at 20% 10%, rgba(223, 245, 154, 0.06), transparent 24rem),
      radial-gradient(ellipse at 80% 90%, rgba(242, 177, 111, 0.04), transparent 20rem),
      linear-gradient(160deg, #0c100d 0%, #0a0e0b 50%, #120f0d 100%);
    background-size: 48px 48px, 48px 48px, auto, auto, auto;
    color: #ebe4d6;
    font: 14px/1.5 "Aptos", "Segoe UI Variable", "Segoe UI", system-ui, sans-serif;
  }

  :global(button),
  :global(input),
  :global(select) {
    font: inherit;
  }

  :global(button:focus-visible),
  :global(select:focus-visible),
  :global(input:focus-visible) {
    outline: 2px solid #dff59a;
    outline-offset: 2px;
    border-radius: 4px;
  }

  :global(::-webkit-scrollbar) {
    width: 6px;
    height: 6px;
  }
  :global(::-webkit-scrollbar-track) {
    background: transparent;
  }
  :global(::-webkit-scrollbar-thumb) {
    background: #2d3329;
    border-radius: 999px;
  }
  :global(*) {
    scrollbar-width: thin;
    scrollbar-color: #2d3329 transparent;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  /* ── App shell ─────────────────────────────────────────── */
  .app {
    display: grid;
    grid-template-rows: 50px 1fr 34px;
    height: 100vh;
    overflow: hidden;
  }

  /* ── Top bar ───────────────────────────────────────────── */
  .top-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 0 14px;
    background: rgba(12, 14, 12, 0.92);
    backdrop-filter: blur(16px);
    border-bottom: 1px solid rgba(223, 245, 154, 0.06);
    z-index: 10;
  }

  .top-bar-left {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }

  .logo-btn {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    padding: 0;
    cursor: pointer;
    flex-shrink: 0;
    transition: background 140ms ease;
  }
  .logo-btn:hover {
    background: rgba(223, 245, 154, 0.08);
  }
  .logo-img {
    width: 28px;
    height: 28px;
    border: 1px solid rgba(223, 245, 154, 0.15);
    border-radius: 6px;
    background: #080b09;
    object-fit: cover;
  }

  .top-bar-context {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }
  .drive-label {
    font-size: 13px;
    font-weight: 600;
    color: #d1c9ba;
  }
  .scan-status {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    font-weight: 700;
    color: #9a948a;
  }

  .top-bar-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .drive-select-label {
    display: block;
  }
  .select-wrap {
    position: relative;
  }
  .select-wrap::after {
    content: "";
    position: absolute;
    right: 8px;
    top: 50%;
    width: 6px;
    height: 6px;
    border-right: 1.5px solid #6b665e;
    border-bottom: 1.5px solid #6b665e;
    transform: translateY(-65%) rotate(45deg);
    pointer-events: none;
  }
  .select-wrap select {
    appearance: none;
    -webkit-appearance: none;
    border: 1px solid rgba(223, 245, 154, 0.08);
    border-radius: 6px;
    background: rgba(26, 30, 26, 0.6);
    color: #d1c9ba;
    font-size: 12px;
    font-weight: 600;
    padding: 5px 26px 5px 10px;
    cursor: pointer;
    transition: border-color 140ms ease;
  }
  .select-wrap select:hover:not(:disabled) {
    border-color: rgba(223, 245, 154, 0.2);
  }
  .select-wrap select:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }

  .stats-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: #9a948a;
  }
  .stats-row .stat strong {
    color: #ebe4d6;
    font-variant-numeric: tabular-nums;
  }

  .icon-btn {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: #9a948a;
    cursor: pointer;
    padding: 0;
    transition: background 140ms ease, color 140ms ease;
  }
  .icon-btn:hover {
    background: rgba(223, 245, 154, 0.08);
    color: #ebe4d6;
  }
  .icon-btn[aria-pressed="true"] {
    color: #dff59a;
    background: rgba(223, 245, 154, 0.1);
  }

  .btn-scan {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    border: 0;
    border-radius: 7px;
    background: #dff59a;
    color: #151b0a;
    font-size: 13px;
    font-weight: 700;
    padding: 0 14px;
    cursor: pointer;
    transition: transform 140ms ease, box-shadow 200ms ease, opacity 140ms ease;
  }
  .btn-scan:hover:not(:disabled) {
    box-shadow: 0 0 20px rgba(223, 245, 154, 0.25);
    transform: translateY(-1px);
  }
  .btn-scan:disabled {
    cursor: not-allowed;
    opacity: 0.45;
  }
  .btn-scan:active:not(:disabled) {
    transform: translateY(0);
  }
  .spin-icon {
    animation: spin 900ms linear infinite;
  }

  .btn-scan-lg {
    height: 44px;
    padding: 0 28px;
    font-size: 15px;
    border-radius: 10px;
  }

  .status-dot {
    width: 6px;
    height: 6px;
    border-radius: 999px;
    background: #6b665e;
    flex-shrink: 0;
  }
  .status-dot.active {
    background: #dff59a;
    box-shadow: 0 0 6px rgba(223, 245, 154, 0.6);
    animation: pulse 1.8s ease infinite;
  }

  /* ── Main area ─────────────────────────────────────────── */
  .main-area {
    display: flex;
    min-height: 0;
    overflow: hidden;
  }
  .content-area {
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    position: relative;
  }

  /* ── Start screen ──────────────────────────────────────── */
  .start-screen {
    display: grid;
    place-items: center;
    height: 100%;
    padding: 2rem;
    box-sizing: border-box;
  }
  .start-content {
    display: grid;
    gap: 2rem;
    max-width: 52rem;
    width: 100%;
    text-align: center;
  }
  .start-content h1 {
    margin: 0;
    font-size: clamp(1.6rem, 3.2vw, 2.6rem);
    font-weight: 800;
    letter-spacing: -0.02em;
    line-height: 1.1;
    background: linear-gradient(135deg, #dff59a 0%, #f2b16f 100%);
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
    background-clip: text;
  }
  .start-content > p {
    margin: 0 auto;
    max-width: 34rem;
    color: #9a948a;
    font-size: clamp(0.9rem, 1.4vw, 1.05rem);
    line-height: 1.6;
  }
  .start-content > p strong {
    color: #dff59a;
    font-weight: 700;
  }

  .drive-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(14rem, 1fr));
    gap: 10px;
    max-width: 40rem;
    margin: 0 auto;
  }

  .drive-card {
    display: flex;
    align-items: center;
    gap: 12px;
    border: 1px solid rgba(223, 245, 154, 0.08);
    border-radius: 10px;
    background: rgba(18, 22, 18, 0.6);
    padding: 14px 16px;
    cursor: pointer;
    text-align: left;
    transition: border-color 140ms ease, background 140ms ease, transform 140ms ease;
  }
  .drive-card:hover:not(.disabled) {
    border-color: rgba(223, 245, 154, 0.25);
    background: rgba(26, 30, 26, 0.5);
    transform: translateY(-1px);
  }
  .drive-card.selected {
    border-color: rgba(223, 245, 154, 0.35);
    background: rgba(223, 245, 154, 0.05);
  }
  .drive-card.disabled {
    cursor: not-allowed;
    opacity: 0.4;
  }
  .drive-card-icon {
    color: #dff59a;
    flex-shrink: 0;
  }
  .drive-card-info {
    display: grid;
    gap: 1px;
    min-width: 0;
  }
  .drive-letter {
    font-size: 16px;
    font-weight: 800;
    color: #ebe4d6;
  }
  .drive-name {
    font-size: 12px;
    color: #9a948a;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .drive-card-meta {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 2px;
    margin-left: auto;
    flex-shrink: 0;
  }
  .drive-fstype {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    padding: 2px 8px;
    border: 1px solid rgba(223, 245, 154, 0.1);
    border-radius: 999px;
    color: #6b665e;
  }
  .drive-card.selected .drive-fstype {
    border-color: rgba(223, 245, 154, 0.25);
    color: #dff59a;
  }
  .drive-free {
    font-size: 10px;
    color: #6b665e;
    font-variant-numeric: tabular-nums;
  }

  .preview-notice {
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: 36rem;
    margin: 0 auto;
    padding: 14px 18px;
    border: 1px solid rgba(255, 177, 153, 0.15);
    border-radius: 10px;
    background: rgba(255, 177, 153, 0.04);
    color: #9a948a;
  }
  .preview-notice svg {
    flex-shrink: 0;
    color: #ffb199;
  }
  .preview-notice p {
    margin: 0;
    font-size: 13px;
  }

  .loading-text,
  .no-drives {
    color: #6b665e;
    margin: 0;
  }

  /* ── Scan screen ───────────────────────────────────────── */
  .scan-screen {
    display: grid;
    place-items: center;
    height: 100%;
  }
  .scan-card {
    display: grid;
    gap: 12px;
    width: min(34rem, 100% - 4rem);
    padding: clamp(1.5rem, 4vw, 2.5rem);
    border: 1px solid rgba(223, 245, 154, 0.1);
    border-radius: 14px;
    background:
      radial-gradient(ellipse at 20% 20%, rgba(223, 245, 154, 0.06), transparent 60%),
      rgba(12, 14, 12, 0.5);
    backdrop-filter: blur(16px);
    text-align: center;
  }
  .scan-header {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #dff59a;
  }
  .scan-big-text {
    font-size: clamp(1.8rem, 3.4vw, 2.8rem);
    font-weight: 850;
    line-height: 1.1;
    color: #ebe4d6;
  }
  .scan-details {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    font-size: 12px;
    color: #6b665e;
  }
  .scan-details span + span::before {
    content: "·";
    margin: 0 4px;
    color: #343b2e;
  }
  .progress-bar {
    height: 6px;
    border-radius: 999px;
    background: rgba(223, 245, 154, 0.08);
    overflow: hidden;
  }
  .progress-fill {
    height: 100%;
    border-radius: inherit;
    background: linear-gradient(90deg, #dff59a, #f2b16f);
    transition: width 480ms cubic-bezier(0.16, 1, 0.3, 1);
    position: relative;
  }
  .progress-fill::after {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(90deg, rgba(255, 255, 255, 0) 0%, rgba(255, 255, 255, 0.3) 50%, rgba(255, 255, 255, 0) 100%);
    animation: sheen 1.5s linear infinite;
    transform: translateX(-100%);
  }
  .progress-label {
    font-size: 11px;
    font-weight: 700;
    color: #9a948a;
    font-variant-numeric: tabular-nums;
  }

  /* ── Error screen ──────────────────────────────────────── */
  .error-screen {
    display: grid;
    place-items: center;
    gap: 12px;
    height: 100%;
    text-align: center;
    padding: 2rem;
  }
  .error-screen h2 {
    margin: 0;
    color: #ffb199;
    font-size: 1.3rem;
  }
  .error-screen p {
    margin: 0;
    color: #9a948a;
    max-width: 28rem;
  }

  /* ── Results screen ────────────────────────────────────── */
  .results-screen {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
  }

  .results-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 18px;
    box-sizing: border-box;
    flex-shrink: 0;
  }
  .toolbar-left,
  .toolbar-right {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .back-btn {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border: 1px solid rgba(223, 245, 154, 0.1);
    border-radius: 6px;
    background: transparent;
    color: #9a948a;
    cursor: pointer;
    flex-shrink: 0;
    padding: 0;
    transition: background 120ms ease, color 120ms ease;
  }
  .back-btn:hover {
    background: rgba(223, 245, 154, 0.08);
    color: #dff59a;
  }

  .breadcrumb {
    display: flex;
    align-items: center;
    gap: 3px;
    min-width: 0;
    overflow: hidden;
  }
  .bc-sep {
    color: #343b2e;
    font-size: 12px;
  }
  .bc-item {
    max-width: 10rem;
    overflow: hidden;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: #d1c9ba;
    cursor: pointer;
    padding: 3px 6px;
    font-size: 12px;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: background 120ms ease, color 120ms ease;
  }
  .bc-item:hover {
    background: rgba(223, 245, 154, 0.08);
    color: #dff59a;
  }

  .tabs {
    display: inline-flex;
    gap: 2px;
    padding: 3px;
    background: rgba(26, 30, 26, 0.6);
    border: 1px solid rgba(223, 245, 154, 0.08);
    border-radius: 8px;
  }
  .tab {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-height: 26px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: #6b665e;
    cursor: pointer;
    font-size: 12px;
    font-weight: 600;
    padding: 4px 12px;
    transition: color 120ms ease, background 120ms ease;
  }
  .tab:hover:not(.active) {
    color: #d1c9ba;
  }
  .tab.active {
    background: rgba(223, 245, 154, 0.1);
    color: #dff59a;
  }

  .results-content {
    flex: 1;
    min-height: 0;
    padding: 0 18px 18px;
    box-sizing: border-box;
    overflow: hidden;
  }

  .context-hint {
    position: absolute;
    left: 18px;
    bottom: 14px;
    font-size: 11px;
    color: #4a463e;
    pointer-events: none;
    z-index: 5;
    transition: opacity 400ms ease;
  }

  /* ── Sidebar ───────────────────────────────────────────── */
  .sidebar {
    flex: 0 0 18rem;
    min-height: 0;
    overflow: hidden;
    border-left: 1px solid rgba(223, 245, 154, 0.06);
    background: rgba(12, 14, 12, 0.85);
    backdrop-filter: blur(12px);
  }

  /* ── Bottom bar ────────────────────────────────────────── */
  .bottom-bar {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 0 14px;
    background: rgba(12, 14, 12, 0.92);
    backdrop-filter: blur(12px);
    border-top: 1px solid rgba(223, 245, 154, 0.06);
    font-size: 11px;
    overflow-x: auto;
  }
  .bottom-stat {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    white-space: nowrap;
  }
  .bottom-stat .label {
    color: #4a463e;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-size: 10px;
  }
  .bottom-stat .value {
    color: #9a948a;
    font-variant-numeric: tabular-nums;
    font-weight: 650;
  }
  .separator {
    width: 1px;
    height: 14px;
    background: rgba(223, 245, 154, 0.06);
  }

  .debug-panel {
    margin-left: auto;
    font-size: 10px;
    color: #4a463e;
  }
  .debug-panel summary {
    cursor: pointer;
    list-style: none;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    transition: color 120ms ease;
  }
  .debug-panel summary:hover {
    color: #9a948a;
  }
  .debug-panel summary::marker {
    content: "▸ ";
  }
  .debug-panel[open] summary::marker {
    content: "▾ ";
  }
  .debug-content {
    display: inline-flex;
    gap: 12px;
    margin-top: 4px;
  }
  .debug-content span {
    white-space: nowrap;
  }
  .debug-content strong {
    color: #6b665e;
    font-variant-numeric: tabular-nums;
  }

  /* ── Animations ────────────────────────────────────────── */
  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }
  @keyframes spin {
    to { transform: rotate(360deg); }
  }
  @keyframes sheen {
    to { transform: translateX(100%); }
  }

  /* ── Responsive ────────────────────────────────────────── */
  @media (max-width: 768px) {
    :global(body) {
      overflow: auto;
    }
    .app {
      grid-template-rows: auto minmax(0, 1fr) auto;
      height: auto;
      min-height: 100vh;
      overflow: visible;
    }
    .top-bar {
      flex-wrap: wrap;
      gap: 8px;
      padding: 8px 12px;
    }
    .top-bar-left {
      width: 100%;
    }
    .top-bar-right {
      width: 100%;
      justify-content: flex-end;
      flex-wrap: wrap;
      gap: 6px;
    }
    .stats-row {
      display: none;
    }
    .sidebar {
      flex: 1 1 auto;
      border-left: 0;
      border-top: 1px solid rgba(223, 245, 154, 0.06);
    }
    .main-area {
      flex-direction: column;
    }
    .results-content {
      min-height: 42vh;
    }
    .results-screen {
      min-height: 100vh;
    }
    .start-content {
      padding: 1rem;
    }
    .drive-grid {
      grid-template-columns: 1fr;
    }
    .bottom-bar {
      flex-wrap: wrap;
      gap: 8px;
      padding: 6px 12px;
    }
    .debug-panel {
      margin-left: 0;
      width: 100%;
    }
    .debug-content {
      flex-wrap: wrap;
    }
  }
</style>
