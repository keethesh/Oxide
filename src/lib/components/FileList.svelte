<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { FilePathRow, FileRow } from "$lib/types";
  import { formatSize } from "$lib/utils";

  const PAGE_SIZE = 200;
  const ROW_HEIGHT = 42;
  const OVERSCAN = 15;

  let {
    scanLoaded,
    rootId,
    onNavigate,
    onOpenInExplorer,
    compact = false
  } = $props<{
    scanLoaded: boolean;
    rootId: number;
    onNavigate: (id: number) => void;
    onOpenInExplorer?: (id: number) => void;
    compact?: boolean;
  }>();

  type HydratedFileRow = FileRow & {
    path?: string;
  };

  let viewport = $state<HTMLDivElement | undefined>(undefined);
  let files = $state<HydratedFileRow[]>([]);
  let loading = $state(false);
  let hasMore = $state(false);
  let error = $state("");
  let lastLoadedRoot = $state<number | null>(null);
  let loadingPathIds = $state(new Set<number>());
  let scrollTop = $state(0);
  let viewportHeight = $state(320);
  let generation = 0;
  let searchQuery = $state("");
  let debouncedQuery = $state("");
  let searchDebounceTimer: ReturnType<typeof setTimeout> | null = null;

  function handleSearchInput(value: string) {
    searchQuery = value;
    if (searchDebounceTimer) {
      clearTimeout(searchDebounceTimer);
    }
    searchDebounceTimer = setTimeout(() => {
      debouncedQuery = value;
      searchDebounceTimer = null;
    }, 150);
  }

  function observeViewport(node: HTMLDivElement) {
    const resizeObserver = new ResizeObserver((entries) => {
      viewportHeight = entries[0]?.contentRect.height ?? 320;
    });
    resizeObserver.observe(node);

    return {
      destroy() {
        resizeObserver.disconnect();
      }
    };
  }

  function addLoadingPaths(fileIds: Iterable<number>) {
    const nextLoading = new Set(loadingPathIds);
    let changed = false;

    for (const id of fileIds) {
      if (!nextLoading.has(id)) {
        nextLoading.add(id);
        changed = true;
      }
    }

    if (changed) {
      loadingPathIds = nextLoading;
    }
  }

  function removeLoadingPaths(fileIds: Iterable<number>) {
    const nextLoading = new Set(loadingPathIds);
    let changed = false;

    for (const id of fileIds) {
      changed = nextLoading.delete(id) || changed;
    }

    if (changed) {
      loadingPathIds = nextLoading;
    }
  }

  async function loadPage(targetRootId: number, reset = false) {
    if (!scanLoaded || loading) {
      return;
    }

    const requestGeneration = generation;
    loading = true;
    try {
      const offset = reset ? 0 : files.length;
      const nextPage = await invoke<FileRow[]>("get_largest_files", {
        rootId: targetRootId,
        offset,
        limit: PAGE_SIZE
      });

      if (requestGeneration !== generation) {
        return;
      }

      const hydratedPage = nextPage.map((file) => ({ ...file }));
      files = reset ? hydratedPage : [...files, ...hydratedPage];
      hasMore = nextPage.length === PAGE_SIZE;
      error = "";
      lastLoadedRoot = targetRootId;
    } catch (err) {
      if (requestGeneration === generation) {
        error = `Failed to load files: ${err}`;
      }
    } finally {
      if (requestGeneration === generation) {
        loading = false;
      }
    }
  }

  async function hydratePaths(fileIds: number[]) {
    const hydratedIds = new Set(files.filter((file) => file.path).map((file) => file.id));
    const missingIds = fileIds.filter(
      (id) => !hydratedIds.has(id) && !loadingPathIds.has(id)
    );
    if (missingIds.length === 0) {
      return;
    }

    const requestGeneration = generation;
    addLoadingPaths(missingIds);

    try {
      const rows = await invoke<FilePathRow[]>("get_file_paths", {
        fileIds: missingIds
      });

      if (requestGeneration !== generation) {
        return;
      }

      const pathById = new Map(rows.map((row) => [row.id, row.path]));
      const unresolvedIds = new Set(missingIds.filter((id) => !pathById.has(id)));
      files = files.map((file) => {
        const path = pathById.get(file.id);
        if (path !== undefined) {
          return file.path === path ? file : { ...file, path };
        }
        if (unresolvedIds.has(file.id) && file.path === undefined) {
          return { ...file, path: "Path unavailable" };
        }
        return file;
      });
    } catch (err) {
      if (requestGeneration === generation) {
        error = `Failed to load file paths: ${err}`;
      }
    } finally {
      if (requestGeneration === generation) {
        removeLoadingPaths(missingIds);
      }
    }
  }

  function pathFor(file: HydratedFileRow) {
    return file.path ?? "Loading path...";
  }

  function handleScroll() {
    scrollTop = viewport?.scrollTop ?? 0;
  }

  const totalHeight = $derived(Math.max(files.length * ROW_HEIGHT, ROW_HEIGHT));
  const startIndex = $derived(Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN));
  const endIndex = $derived(
    Math.min(files.length, Math.ceil((scrollTop + viewportHeight) / ROW_HEIGHT) + OVERSCAN)
  );
  const renderedRows = $derived.by(() => {
    const start = startIndex;
    const filtered = debouncedQuery
      ? files.filter((f) => f.name.toLowerCase().includes(debouncedQuery.toLowerCase()))
      : files;
    return filtered.slice(start, endIndex).map((file, index) => ({
      file,
      top: (start + index) * ROW_HEIGHT
    }));
  });

  const filteredCount = $derived(
    debouncedQuery
      ? files.filter((f) => f.name.toLowerCase().includes(debouncedQuery.toLowerCase())).length
      : files.length
  );

  $effect(() => {
    const loaded = scanLoaded;
    const currentRoot = rootId;

    if (!loaded) {
      generation += 1;
      files = [];
      hasMore = false;
      error = "";
      lastLoadedRoot = null;
      loadingPathIds = new Set();
      scrollTop = 0;
      if (viewport) {
        viewport.scrollTop = 0;
      }
      return;
    }

    if (currentRoot === lastLoadedRoot) {
      return;
    }

    generation += 1;
    files = [];
    hasMore = false;
    error = "";
    loadingPathIds = new Set();
    lastLoadedRoot = currentRoot;
    scrollTop = 0;
    if (viewport) {
      viewport.scrollTop = 0;
    }

    untrack(() => {
      void loadPage(currentRoot, true);
    });
  });

  $effect(() => {
    const currentRoot = rootId;
    const nearEnd = endIndex >= files.length - OVERSCAN * 2;

    if (!scanLoaded || !hasMore || loading || currentRoot !== lastLoadedRoot || !nearEnd) {
      return;
    }

    untrack(() => {
      void loadPage(currentRoot, false);
    });
  });

  $effect(() => {
    const currentRoot = rootId;
    const visibleIds = renderedRows.map(({ file }) => file.id);

    if (!scanLoaded || currentRoot !== lastLoadedRoot || visibleIds.length === 0) {
      return;
    }

    untrack(() => {
      void hydratePaths(visibleIds);
    });
  });
</script>

<div class:compact class="file-list">
  <div class="heading">
    <div class="heading-left">
      <h2>{compact ? "Top Files" : "Largest Files"}</h2>
      <p>{files.length ? `${filteredCount.toLocaleString()} of ${files.length.toLocaleString()} files` : "Selected scope"}</p>
    </div>
    {#if !compact && files.length > 0}
      <div class="search-box">
        <input
          type="text"
          placeholder="Filter files..."
          value={searchQuery}
          oninput={(e) => handleSearchInput(e.currentTarget.value)}
          class="search-input"
        />
        {#if searchQuery}
          <button class="search-clear" onclick={() => { searchQuery = ""; debouncedQuery = ""; if (searchDebounceTimer) { clearTimeout(searchDebounceTimer); searchDebounceTimer = null; } }}>×</button>
        {/if}
      </div>
    {/if}
  </div>

  {#if !scanLoaded}
    <p class="message">Run a scan to load file data.</p>
  {:else if error}
    <p class="message error">{error}</p>
  {:else}
    <div class="table">
      <div class="table-head">
        <span>Name</span>
        <span>Size</span>
        {#if !compact}
          <span>Path</span>
        {/if}
      </div>

      {#if files.length === 0 && loading}
        <p class="message">Loading files...</p>
      {:else if files.length === 0}
        <p class="message">No files found in this location.</p>
      {:else}
        <div class="viewport" bind:this={viewport} use:observeViewport onscroll={handleScroll}>
          <div class="canvas" style={`height: ${totalHeight}px`}>
            {#each renderedRows as { file, top } (file.id)}
              <div
                class="row"
                style={`top: ${top}px; height: ${ROW_HEIGHT}px`}
                role="listitem"
                oncontextmenu={(e) => { e.preventDefault(); onOpenInExplorer?.(file.id); }}
              >
                <button class="name-cell" onclick={() => onNavigate(file.parent_id)}>
                  <span class="name">{file.name}</span>
                  {#if file.is_hidden}
                    <span class="badge">Hidden</span>
                  {/if}
                </button>
                <span class="size">{formatSize(file.size)}</span>
                {#if !compact}
                  <span class="path">{pathFor(file)}</span>
                {/if}
              </div>
            {/each}
          </div>
        </div>

        {#if loading}
          <p class="message">Loading more files...</p>
        {:else if hasMore}
          <p class="message">Scroll to load more.</p>
        {/if}
      {/if}
    </div>
  {/if}
</div>



<style>
  .file-list {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    width: 100%;
    height: 100%;
    color: #ebe4d8;
    min-height: 0;
  }

  .file-list.compact {
    gap: 0.6rem;
  }

  .heading {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
    min-height: 28px;
  }

  .heading-left {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .heading h2,
  .heading p {
    margin: 0;
  }

  .heading h2 {
    font-size: 0.74rem;
    color: #fbf6eb;
    font-weight: 850;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .heading p,
  .message {
    font-size: 0.76rem;
    color: #a8a094;
  }

  .error {
    color: #ffb199;
  }

  .table {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    border: 1px solid rgba(223, 245, 154, 0.08);
    border-radius: 8px;
    overflow: hidden;
    background:
      linear-gradient(180deg, rgba(223, 245, 154, 0.028), transparent),
      oklch(20% 0.018 125);
  }

  .table-head,
  .row {
    display: grid;
    grid-template-columns: minmax(120px, 1.35fr) 92px minmax(160px, 2fr);
    gap: 0.75rem;
    align-items: center;
    padding: 0 0.9rem;
    box-sizing: border-box;
  }

  .compact .table-head,
  .compact .row {
    grid-template-columns: minmax(0, 1fr) 76px;
    gap: 0.55rem;
    padding: 0 0.7rem;
  }

  .table-head {
    min-height: 36px;
    border-bottom: 1px solid rgba(223, 245, 154, 0.08);
    color: #a8a094;
    font-size: 0.72rem;
    font-weight: 800;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .compact .table-head {
    min-height: 32px;
  }

  .viewport {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .canvas {
    position: relative;
    width: 100%;
  }

  .row {
    position: absolute;
    left: 0;
    right: 0;
    border-bottom: 1px solid rgba(223, 245, 154, 0.045);
    transition: background 140ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  .row:hover {
    background: oklch(24% 0.019 125);
  }

  .row::before {
    content: "";
    position: absolute;
    left: 0;
    top: 9px;
    bottom: 9px;
    width: 2px;
    border-radius: 0 2px 2px 0;
    background: transparent;
    transition: background 140ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  .row:hover::before {
    background: #dff59a;
  }

  .name-cell {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
    border: none;
    background: transparent;
    color: #dff59a;
    cursor: pointer;
    padding: 0;
    font: inherit;
    font-weight: 650;
    text-align: left;
    transition: color 140ms cubic-bezier(0.16, 1, 0.3, 1), transform 140ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  .name-cell:hover {
    color: #f2b16f;
    transform: translateX(2px);
  }

  .compact .name-cell {
    color: #ebe4d8;
  }

  .name,
  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .size {
    color: #fbf6eb;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .compact .size {
    color: #dff59a;
    font-size: 0.78rem;
    text-align: right;
  }

  .path {
    color: #a8a094;
  }

  .badge {
    display: inline-flex;
    border: 1px solid rgba(255, 177, 153, 0.35);
    border-radius: 4px;
    padding: 0.08rem 0.45rem;
    color: #ffb199;
    font-size: 0.68rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    white-space: nowrap;
  }

  .search-box {
    position: relative;
    display: flex;
    align-items: center;
  }

  .search-input {
    width: 160px;
    appearance: none;
    border: 1px solid rgba(223, 245, 154, 0.12);
    border-radius: 6px;
    background: var(--surface-1);
    color: #e8e2d6;
    font-size: 0.72rem;
    padding: 0.28rem 1.8rem 0.28rem 0.6rem;
    transition: border-color 140ms var(--ease), width 200ms var(--ease);
  }

  .search-input::placeholder {
    color: var(--dim);
  }

  .search-input:focus {
    outline: none;
    border-color: rgba(223, 245, 154, 0.3);
    width: 200px;
  }

  .search-clear {
    position: absolute;
    right: 6px;
    border: none;
    background: transparent;
    color: var(--dim);
    cursor: pointer;
    font-size: 1rem;
    line-height: 1;
    padding: 0 2px;
  }

  .search-clear:hover {
    color: var(--muted);
  }

  @media (max-width: 760px) {
    .table-head,
    .row {
      grid-template-columns: minmax(120px, 1fr) 80px;
    }

    .path {
      display: none;
    }
  }
</style>
