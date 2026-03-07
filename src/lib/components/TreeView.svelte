<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { NodeSummary } from "$lib/types";

  interface VisibleNode extends NodeSummary {
    depth: number;
  }

  let {
    scanLoaded,
    scanRootId,
    selectedId,
    onSelect
  } = $props<{
    scanLoaded: boolean;
    scanRootId: number;
    selectedId: number;
    onSelect: (id: number) => void;
  }>();

  let childrenByParent = $state<Record<number, NodeSummary[]>>({});
  let expanded = $state<number[]>([]);
  let loadingIds = $state<number[]>([]);
  let error = $state("");

  function isExpanded(id: number) {
    return expanded.includes(id);
  }

  function isLoading(id: number) {
    return loadingIds.includes(id);
  }

  async function ensureChildren(id: number) {
    if (childrenByParent[id] || isLoading(id)) {
      return;
    }

    loadingIds = [...loadingIds, id];
    try {
      childrenByParent = {
        ...childrenByParent,
        [id]: await invoke<NodeSummary[]>("get_children", { nodeId: id })
      };
      error = "";
    } catch (err) {
      error = `Failed to load folders: ${err}`;
    } finally {
      loadingIds = loadingIds.filter((value) => value !== id);
    }
  }

  async function toggleExpand(node: NodeSummary) {
    if (!node.is_dir) {
      return;
    }

    if (isExpanded(node.id)) {
      expanded = expanded.filter((id) => id !== node.id);
      return;
    }

    await ensureChildren(node.id);
    expanded = [...expanded, node.id];
  }

  function visibleNodes(parentId = scanRootId, depth = 0): VisibleNode[] {
    const nodes = childrenByParent[parentId] ?? [];
    const flattened: VisibleNode[] = [];

    for (const node of nodes) {
      flattened.push({ ...node, depth });
      if (node.is_dir && isExpanded(node.id)) {
        flattened.push(...visibleNodes(node.id, depth + 1));
      }
    }

    return flattened;
  }

  $effect(() => {
    const loaded = scanLoaded;
    const root = scanRootId;
    childrenByParent = {};
    expanded = [];
    error = "";

    if (loaded) {
      ensureChildren(root);
      expanded = [root];
    }
  });
</script>

<div class="tree-view">
  <div class="heading">
    <h2>Folders</h2>
    <p>Browse the scanned hierarchy.</p>
  </div>

  {#if error}
    <p class="message error">{error}</p>
  {/if}

  {#if !scanLoaded}
    <p class="message">Run a scan to load the folder tree.</p>
  {:else if !childrenByParent[scanRootId] && isLoading(scanRootId)}
    <p class="message">Loading folders...</p>
  {:else}
    <div class="rows">
      {#each visibleNodes() as node (node.id)}
        <div
          class:selected={selectedId === node.id}
          class="row"
          style={`padding-left: ${0.75 + node.depth * 1}rem`}
        >
          <button
            class="expander"
            disabled={!node.is_dir}
            onclick={() => toggleExpand(node)}
            aria-label={node.is_dir ? "Toggle folder" : "File"}
          >
            {#if node.is_dir}
              {isExpanded(node.id) ? "▾" : "▸"}
            {:else}
              ""
            {/if}
          </button>
          <button class="node" onclick={() => onSelect(node.id)}>
            <span class="name">{node.name}</span>
            <span class="meta">{formatSize(node.size)}</span>
          </button>
        </div>
      {/each}
    </div>
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
  .tree-view {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    padding: 1rem;
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

  .rows {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.02);
  }

  .row.selected {
    background: rgba(255, 62, 0, 0.18);
  }

  .expander,
  .node {
    border: none;
    color: inherit;
    cursor: pointer;
  }

  .expander {
    width: 1.5rem;
    background: transparent;
    padding: 0.35rem 0;
    color: #a1a1a1;
  }

  .expander:disabled {
    cursor: default;
    opacity: 0.3;
  }

  .node {
    flex: 1;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.75rem;
    background: transparent;
    padding: 0.4rem 0.6rem 0.4rem 0;
    text-align: left;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    color: #999;
    font-size: 0.8rem;
    white-space: nowrap;
  }
</style>
