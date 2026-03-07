<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { FileRow } from "$lib/types";

  const PAGE_SIZE = 200;

  let {
    scanLoaded,
    rootId,
    onNavigate
  } = $props<{
    scanLoaded: boolean;
    rootId: number;
    onNavigate: (id: number) => void;
  }>();

  let files = $state<FileRow[]>([]);
  let loading = $state(false);
  let hasMore = $state(false);
  let error = $state("");
  let lastLoadedRoot = $state<number | null>(null);

  async function loadPage(reset = false) {
    if (!scanLoaded || loading) {
      return;
    }

    loading = true;
    try {
      const offset = reset ? 0 : files.length;
      const nextPage = await invoke<FileRow[]>("get_largest_files", {
        rootId,
        offset,
        limit: PAGE_SIZE
      });

      files = reset ? nextPage : [...files, ...nextPage];
      hasMore = nextPage.length === PAGE_SIZE;
      error = "";
      lastLoadedRoot = rootId;
    } catch (err) {
      error = `Failed to load files: ${err}`;
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    const loaded = scanLoaded;
    const currentRoot = rootId;
    if (!loaded) {
      files = [];
      hasMore = false;
      error = "";
      lastLoadedRoot = null;
      return;
    }

    if (currentRoot === lastLoadedRoot) {
      return;
    }

    files = [];
    hasMore = false;
    error = "";
    loadPage(true);
  });
</script>

<div class="file-list">
  <div class="heading">
    <h2>Largest Files</h2>
    <p>Top files within the selected subtree.</p>
  </div>

  {#if !scanLoaded}
    <p class="message">Run a scan to load file data.</p>
  {:else if error}
    <p class="message error">{error}</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>Name</th>
          <th>Size</th>
          <th>Path</th>
        </tr>
      </thead>
      <tbody>
        {#if files.length === 0 && loading}
          <tr>
            <td colspan="3" class="message">Loading files...</td>
          </tr>
        {:else if files.length === 0}
          <tr>
            <td colspan="3" class="message">No files found in this location.</td>
          </tr>
        {:else}
          {#each files as file (file.id)}
            <tr>
              <td>
                <button class="row-link" onclick={() => onNavigate(file.parent_id)}>
                  {file.name}
                </button>
              </td>
              <td>{formatSize(file.size)}</td>
              <td class="path">{file.path}</td>
            </tr>
          {/each}
        {/if}
      </tbody>
    </table>

    {#if hasMore}
      <button class="load-more" disabled={loading} onclick={() => loadPage(false)}>
        {loading ? "Loading..." : "Load More"}
      </button>
    {/if}
  {/if}
</div>

<script lang="ts" module>
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
</script>

<style>
  .file-list {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    width: 100%;
    color: #d7d7d7;
  }

  .heading h2,
  .heading p {
    margin: 0;
  }

  .heading h2 {
    font-size: 1rem;
    color: #f7f7f7;
  }

  .heading p,
  .message {
    font-size: 0.85rem;
    color: #949494;
  }

  .error {
    color: #ff8f7a;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    background: rgba(255, 255, 255, 0.02);
    border: 1px solid #2f2f2f;
    border-radius: 12px;
    overflow: hidden;
  }

  th,
  td {
    padding: 0.8rem;
    text-align: left;
    border-bottom: 1px solid #2b2b2b;
    vertical-align: top;
  }

  th {
    color: #999;
    font-size: 0.78rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  tbody tr:last-child td {
    border-bottom: none;
  }

  .row-link {
    border: none;
    background: transparent;
    color: #ff8b67;
    cursor: pointer;
    padding: 0;
    font: inherit;
    text-align: left;
  }

  .path {
    color: #a6a6a6;
    word-break: break-word;
  }

  .load-more {
    align-self: flex-start;
    border: 1px solid #ff5d2a;
    background: #ff5d2a;
    color: #fff;
    border-radius: 999px;
    padding: 0.65rem 1rem;
    cursor: pointer;
    font-weight: 600;
  }

  .load-more:disabled {
    cursor: wait;
    opacity: 0.7;
  }
</style>
