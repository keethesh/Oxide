<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { ChildPage, NodeSummary } from "$lib/types";

  const PAGE_SIZE = 200;
  const ROW_HEIGHT = 34;
  const OVERSCAN = 20;

  interface LoadedChildren {
    items: NodeSummary[];
    total: number;
    nextOffset: number | null;
  }

  type VisibleRow =
    | {
        key: string;
        kind: "node";
        depth: number;
        node: NodeSummary;
      }
    | {
        key: string;
        kind: "load-more";
        depth: number;
        parentId: number;
        remaining: number;
      };

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

  let viewport = $state<HTMLDivElement | undefined>(undefined);
  let childrenByParent = $state(new Map<number, LoadedChildren>());
  let expanded = $state(new Set<number>());
  let loadingParents = $state(new Set<number>());
  let error = $state("");
  let scrollTop = $state(0);
  let viewportHeight = $state(320);
  let generation = 0;

  function isExpanded(id: number) {
    return expanded.has(id);
  }

  function isLoading(id: number) {
    return loadingParents.has(id);
  }

  function getChildren(parentId: number) {
    return childrenByParent.get(parentId) ?? null;
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

  function setChildren(parentId: number, nextPage: LoadedChildren) {
    const nextChildren = new Map(childrenByParent);
    nextChildren.set(parentId, nextPage);
    childrenByParent = nextChildren;
  }

  function deleteChildren(parentIds: Iterable<number>) {
    const nextChildren = new Map(childrenByParent);
    let changed = false;

    for (const parentId of parentIds) {
      changed = nextChildren.delete(parentId) || changed;
    }

    if (changed) {
      childrenByParent = nextChildren;
    }
  }

  function addLoading(parentId: number) {
    if (loadingParents.has(parentId)) {
      return;
    }

    const nextLoading = new Set(loadingParents);
    nextLoading.add(parentId);
    loadingParents = nextLoading;
  }

  function removeLoading(parentIds: Iterable<number>) {
    const nextLoading = new Set(loadingParents);
    let changed = false;

    for (const parentId of parentIds) {
      changed = nextLoading.delete(parentId) || changed;
    }

    if (changed) {
      loadingParents = nextLoading;
    }
  }

  function addExpanded(parentId: number) {
    if (expanded.has(parentId)) {
      return;
    }

    const nextExpanded = new Set(expanded);
    nextExpanded.add(parentId);
    expanded = nextExpanded;
  }

  function removeExpanded(parentIds: Iterable<number>) {
    const nextExpanded = new Set(expanded);
    let changed = false;

    for (const parentId of parentIds) {
      changed = nextExpanded.delete(parentId) || changed;
    }

    if (changed) {
      expanded = nextExpanded;
    }
  }

  async function loadChildren(parentId: number, reset = false) {
    if (!scanLoaded) {
      return;
    }

    const existing = getChildren(parentId);
    if (!reset && (isLoading(parentId) || existing?.nextOffset === null)) {
      return;
    }

    const requestGeneration = generation;
    const offset = reset || !existing ? 0 : existing.items.length;

    addLoading(parentId);
    try {
      const page = await invoke<ChildPage>("get_children", {
        nodeId: parentId,
        offset,
        limit: PAGE_SIZE
      });

      if (requestGeneration !== generation) {
        return;
      }

      const current = reset ? null : getChildren(parentId);
      setChildren(parentId, {
        items: current ? [...current.items, ...page.items] : page.items,
        total: page.total,
        nextOffset: page.next_offset
      });
      error = "";
    } catch (err) {
      if (requestGeneration === generation) {
        error = `Failed to load folders: ${err}`;
      }
    } finally {
      if (requestGeneration === generation) {
        removeLoading([parentId]);
      }
    }
  }

  function collapseBranch(parentId: number) {
    const stack = [parentId];
    const idsToDelete: number[] = [];

    while (stack.length > 0) {
      const currentId = stack.pop()!;
      idsToDelete.push(currentId);
      const page = getChildren(currentId);
      if (page) {
        for (const child of page.items) {
          if (child.is_dir) {
            stack.push(child.id);
          }
        }
      }

    }

    deleteChildren(idsToDelete);
    removeLoading(idsToDelete);
    removeExpanded(idsToDelete);
  }

  async function toggleExpand(node: NodeSummary) {
    if (!node.is_dir || node.child_count === 0) {
      return;
    }

    if (isExpanded(node.id)) {
      collapseBranch(node.id);
      return;
    }

    if (!getChildren(node.id)) {
      await loadChildren(node.id, true);
    }
    addExpanded(node.id);
  }

  function buildVisibleRows(parentId: number, depth: number): VisibleRow[] {
    const page = getChildren(parentId);
    if (!page) {
      return [];
    }

    const rows: VisibleRow[] = [];
    for (const node of page.items) {
      rows.push({
        key: `node:${node.id}`,
        kind: "node",
        depth,
        node
      });

      if (node.is_dir && isExpanded(node.id)) {
        rows.push(...buildVisibleRows(node.id, depth + 1));
      }
    }

    if (page.nextOffset !== null) {
      rows.push({
        key: `more:${parentId}:${page.items.length}`,
        kind: "load-more",
        parentId,
        depth,
        remaining: page.total - page.items.length
      });
    }

    return rows;
  }

  const visibleRows = $derived.by(() =>
    scanLoaded ? buildVisibleRows(scanRootId, 0) : []
  );
  const totalHeight = $derived(Math.max(visibleRows.length * ROW_HEIGHT, ROW_HEIGHT));
  const startIndex = $derived(Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN));
  const endIndex = $derived(
    Math.min(
      visibleRows.length,
      Math.ceil((scrollTop + viewportHeight) / ROW_HEIGHT) + OVERSCAN
    )
  );
  const renderedRows = $derived.by(() => {
    const start = startIndex;
    return visibleRows.slice(start, endIndex).map((row, index) => ({
      row,
      top: (start + index) * ROW_HEIGHT
    }));
  });

  function handleScroll() {
    scrollTop = viewport?.scrollTop ?? 0;
  }

  $effect(() => {
    const loaded = scanLoaded;
    const root = scanRootId;

    untrack(() => {
      generation += 1;
      childrenByParent = new Map();
      expanded = loaded ? new Set([root]) : new Set();
      loadingParents = new Set();
      error = "";
      scrollTop = 0;

      if (viewport) {
        viewport.scrollTop = 0;
      }

      if (loaded) {
        void loadChildren(root, true);
      }
    });
  });
</script>

<div class="tree-view">
  <div class="heading">
    <h2>Folders</h2>
    <p>Hierarchy</p>
  </div>

  {#if error}
    <p class="message error">{error}</p>
  {/if}

  {#if !scanLoaded}
    <p class="message">Run a scan to load the folder tree.</p>
  {:else if !childrenByParent.has(scanRootId) && isLoading(scanRootId)}
    <p class="message">Loading folders...</p>
  {:else if visibleRows.length === 0}
    <p class="message">No folders found in this location.</p>
  {:else}
    <div class="viewport" bind:this={viewport} use:observeViewport onscroll={handleScroll}>
      <div class="canvas" style={`height: ${totalHeight}px`}>
        {#each renderedRows as { row, top } (row.key)}
          {#if row.kind === "node"}
            <div
              class:selected={selectedId === row.node.id}
              class:hidden-row={row.node.is_hidden}
              class="row"
              style={`top: ${top}px; height: ${ROW_HEIGHT}px; padding-left: ${0.75 + row.depth}rem;`}
            >
              <button
                class="expander"
                disabled={!row.node.is_dir || row.node.child_count === 0}
                onclick={() => toggleExpand(row.node)}
                aria-label={row.node.is_dir ? "Toggle folder" : "File"}
              >
                {#if row.node.is_dir && row.node.child_count > 0}
                  {isExpanded(row.node.id) ? "▾" : "▸"}
                {/if}
              </button>

              <button class="node" onclick={() => onSelect(row.node.id)}>
                <span class="name-wrap">
                  <span class="name">{row.node.name}</span>
                  {#if row.node.is_hidden}
                    <span class="badge">Hidden</span>
                  {/if}
                </span>
                <span class="meta">{formatSize(row.node.size)}</span>
              </button>
            </div>
          {:else}
            <div
              class="row load-more-row"
              style={`top: ${top}px; height: ${ROW_HEIGHT}px; padding-left: ${0.75 + row.depth}rem;`}
            >
              <span class="expander spacer"></span>
              <button
                class="load-more"
                disabled={isLoading(row.parentId)}
                onclick={() => loadChildren(row.parentId, false)}
              >
                {#if isLoading(row.parentId)}
                  Loading more...
                {:else}
                  Load More ({row.remaining.toLocaleString()} remaining)
                {/if}
              </button>
            </div>
          {/if}
        {/each}
      </div>
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
    height: 100%;
    padding: 16px 20px;
    color: #ebe4d8;
    box-sizing: border-box;
  }

  .heading {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
    min-height: 28px;
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

  .viewport {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: auto;
    border: 1px solid rgba(223, 245, 154, 0.08);
    border-radius: 8px;
    background:
      linear-gradient(180deg, rgba(223, 245, 154, 0.028), transparent),
      oklch(20% 0.018 125);
  }

  .canvas {
    position: relative;
    width: 100%;
  }

  .row {
    position: absolute;
    left: 0;
    right: 0;
    display: flex;
    align-items: center;
    gap: 0.25rem;
    box-sizing: border-box;
    border-bottom: 1px solid rgba(223, 245, 154, 0.045);
    transition: background 140ms cubic-bezier(0.16, 1, 0.3, 1), color 140ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  .row:hover {
    background: oklch(24% 0.019 125);
  }

  .row.selected {
    background:
      linear-gradient(90deg, rgba(223, 245, 154, 0.16), rgba(223, 245, 154, 0.04));
    color: #fbf6eb;
  }

  .row.hidden-row {
    border-left: 1px dashed rgba(255, 177, 153, 0.5);
  }

  .expander,
  .node,
  .load-more {
    border: none;
    color: inherit;
    cursor: pointer;
  }

  .expander {
    width: 1.5rem;
    background: transparent;
    padding: 0;
    color: #a8a094;
    flex-shrink: 0;
    transition: color 140ms cubic-bezier(0.16, 1, 0.3, 1), transform 140ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  .expander:hover:not(:disabled) {
    color: #dff59a;
    transform: scale(1.08);
  }

  .expander:disabled {
    cursor: default;
    opacity: 0.3;
  }

  .spacer {
    pointer-events: none;
    opacity: 0;
  }

  .node,
  .load-more {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    width: 100%;
    background: transparent;
    padding: 0.45rem 0.65rem 0.45rem 0;
    text-align: left;
    font: inherit;
  }

  .name-wrap {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-width: 0;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .node:hover .name {
    color: #fbf6eb;
  }

  .badge {
    border: 1px solid rgba(255, 180, 155, 0.35);
    border-radius: 4px;
    padding: 0.08rem 0.45rem;
    color: #ffb199;
    font-size: 0.68rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    white-space: nowrap;
  }

  .meta {
    color: #a8a094;
    font-size: 0.8rem;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .load-more-row {
    background: rgba(223, 245, 154, 0.025);
  }

  .load-more {
    color: #dff59a;
    font-weight: 700;
    transition: color 140ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  .load-more:hover:not(:disabled) {
    color: #f2b16f;
  }
</style>
