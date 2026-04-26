<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { TreemapRect } from "$lib/types";

  const LABEL_MIN_WIDTH = 72;
  const LABEL_MIN_HEIGHT = 22;
  const HOVER_BORDER = "#dff59a";
  const TREEMAP_COLORS = [
    [45, 74, 62],
    [61, 90, 78],
    [74, 107, 92],
    [90, 74, 62],
    [107, 90, 78],
    [74, 74, 90],
    [90, 90, 106],
    [62, 74, 90],
    [90, 74, 90],
    [74, 90, 74],
    [90, 106, 90],
    [121, 78, 61]
  ];

  let {
    rootId = 0,
    onNavigate
  } = $props<{
    rootId: number;
    onNavigate: (id: number) => void;
  }>();

  let canvas: HTMLCanvasElement;
  let container: HTMLDivElement;
  let ctx: CanvasRenderingContext2D | null = null;
  let layout = $state<TreemapRect[]>([]);
  let width = $state(0);
  let height = $state(0);
  let pixelRatio = $state(1);
  let hoveredRect = $state<TreemapRect | null>(null);
  let layoutLoading = $state(false);
  let layoutError = $state("");
  let requestId = 0;
  let layoutFrame = 0;
  let renderFrame = 0;
  let lastLayoutKey = "";
  let pointerActive = false;
  let pointerX = 0;
  let pointerY = 0;

  function bucket(value: number) {
    return Math.max(32, Math.round(value / 32) * 32);
  }

  async function updateLayout(targetRootId: number, targetWidth: number, targetHeight: number) {
    if (!canvas || targetWidth <= 0 || targetHeight <= 0) {
      return;
    }

    const layoutKey = `${targetRootId}:${bucket(targetWidth)}x${bucket(targetHeight)}`;
    if (layoutKey === lastLayoutKey && layout.length > 0) {
      return;
    }

    const currentRequestId = ++requestId;
    layoutLoading = true;
    layoutError = "";

    try {
      const nextLayout = await invoke<TreemapRect[]>("get_treemap_layout", {
        rootId: targetRootId,
        width: targetWidth,
        height: targetHeight
      });

      if (currentRequestId !== requestId) {
        return;
      }

      layout = nextLayout;
      hoveredRect = null;
      lastLayoutKey = layoutKey;
      scheduleRender();
    } catch (error) {
      if (currentRequestId !== requestId) {
        return;
      }

      layout = [];
      hoveredRect = null;
      layoutError = `Failed to build treemap: ${error}`;
      scheduleRender();
    } finally {
      if (currentRequestId === requestId) {
        layoutLoading = false;
      }
    }
  }

  function scheduleLayout(targetRootId: number, targetWidth: number, targetHeight: number) {
    if (layoutFrame) {
      cancelAnimationFrame(layoutFrame);
    }

    layoutFrame = requestAnimationFrame(() => {
      layoutFrame = 0;
      void updateLayout(targetRootId, targetWidth, targetHeight);
    });
  }

  function findRectAt(x: number, y: number): TreemapRect | null {
    for (let index = layout.length - 1; index >= 0; index -= 1) {
      const rect = layout[index];
      if (x >= rect.x && x <= rect.x + rect.w && y >= rect.y && y <= rect.y + rect.h) {
        return rect;
      }
    }

    return null;
  }

  function scheduleRender() {
    if (renderFrame) {
      return;
    }

    renderFrame = requestAnimationFrame(() => {
      renderFrame = 0;
      const nextHoveredRect = pointerActive ? findRectAt(pointerX, pointerY) : null;
      if (!sameRect(nextHoveredRect, hoveredRect)) {
        hoveredRect = nextHoveredRect;
      }
      render();
    });
  }

  function handleMouseMove(event: MouseEvent) {
    const bounds = canvas.getBoundingClientRect();
    pointerActive = true;
    pointerX = event.clientX - bounds.left;
    pointerY = event.clientY - bounds.top;
    scheduleRender();
  }

  function handleMouseLeave() {
    pointerActive = false;
    scheduleRender();
  }

  function handleClick() {
    if (hoveredRect?.kind === "node" && hoveredRect.id !== null) {
      onNavigate(hoveredRect.id);
    }
  }

  function colorFor(rect: TreemapRect, hovered: boolean) {
    if (rect.kind === "overflow") {
      return hovered ? "#74584f" : "#4d3934";
    }

    const source = TREEMAP_COLORS[Math.abs(rect.id ?? 0) % TREEMAP_COLORS.length];
    const lift = hovered ? 24 : 0;
    return `rgb(${Math.min(source[0] + lift, 255)} ${Math.min(source[1] + lift, 255)} ${Math.min(source[2] + lift, 255)})`;
  }

  function render() {
    if (!ctx) {
      return;
    }

    ctx.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
    ctx.clearRect(0, 0, width, height);

    for (const rect of layout) {
      const isHovered = sameRect(rect, hoveredRect);
      const fill = colorFor(rect, isHovered);

      ctx.fillStyle = fill;
      ctx.fillRect(rect.x, rect.y, rect.w, rect.h);

      ctx.strokeStyle = isHovered ? HOVER_BORDER : "rgba(12, 14, 12, 0.58)";
      ctx.lineWidth = isHovered ? 2 : 1;
      ctx.strokeRect(rect.x, rect.y, rect.w, rect.h);

      if (rect.w >= LABEL_MIN_WIDTH && rect.h >= LABEL_MIN_HEIGHT) {
        ctx.save();
        ctx.beginPath();
        ctx.rect(rect.x + 2, rect.y + 2, rect.w - 4, rect.h - 4);
        ctx.clip();
        ctx.fillStyle = "rgba(255, 250, 244, 0.94)";
        ctx.font = "600 12px 'Segoe UI Variable', 'Segoe UI', sans-serif";
        ctx.textBaseline = "top";
        ctx.fillText(rect.label, rect.x + 8, rect.y + 6, Math.max(0, rect.w - 16));
        ctx.restore();
      }
    }
  }

  function sameRect(left: TreemapRect | null, right: TreemapRect | null) {
    return left?.id === right?.id && left?.kind === right?.kind && left?.label === right?.label;
  }

  onMount(() => {
    ctx = canvas.getContext("2d");

    const resizeObserver = new ResizeObserver((entries) => {
      width = entries[0]?.contentRect.width ?? 0;
      height = entries[0]?.contentRect.height ?? 0;
      pixelRatio = Math.max(1, window.devicePixelRatio || 1);
    });

    resizeObserver.observe(container);

    return () => {
      if (layoutFrame) {
        cancelAnimationFrame(layoutFrame);
      }
      if (renderFrame) {
        cancelAnimationFrame(renderFrame);
      }
      resizeObserver.disconnect();
    };
  });

  $effect(() => {
    const currentRootId = rootId;
    const currentWidth = width;
    const currentHeight = height;

    if (currentWidth <= 0 || currentHeight <= 0) {
      return;
    }

    untrack(() => {
      scheduleLayout(currentRootId, currentWidth, currentHeight);
    });
  });

  $effect(() => {
    layout;
    hoveredRect;
    width;
    height;
    pixelRatio;
    scheduleRender();
  });

  const canvasWidth = $derived(Math.max(1, Math.round(width * pixelRatio)));
  const canvasHeight = $derived(Math.max(1, Math.round(height * pixelRatio)));
</script>

<div class="treemap-container" bind:this={container}>
  {#if layoutLoading}
    <div class="overlay">Rendering treemap...</div>
  {:else if layoutError}
    <div class="overlay error">{layoutError}</div>
  {/if}

  {#if hoveredRect}
    <div class="hover-card">
      <span>{hoveredRect.kind === "overflow" ? "Grouped files" : "Open folder"}</span>
      <strong>{hoveredRect.label}</strong>
    </div>
  {/if}

  <canvas
    bind:this={canvas}
    width={canvasWidth}
    height={canvasHeight}
    onmousemove={handleMouseMove}
    onmouseleave={handleMouseLeave}
    onclick={handleClick}
  ></canvas>
</div>

<style>
  .treemap-container {
    width: 100%;
    height: 100%;
    background:
      linear-gradient(135deg, rgba(223, 245, 154, 0.035), transparent 42%),
      #10130f;
    overflow: hidden;
    position: relative;
    border: 1px solid rgba(223, 245, 154, 0.08);
    border-radius: 12px;
    box-shadow: inset 0 0 0 1px rgba(255, 252, 239, 0.018);
  }

  .overlay {
    position: absolute;
    top: 0.7rem;
    left: 0.7rem;
    z-index: 1;
    border: 1px solid rgba(223, 245, 154, 0.1);
    border-radius: 999px;
    background: rgba(15, 18, 14, 0.46);
    padding: 0.28rem 0.55rem;
    color: #fbf6eb;
    font-size: 0.68rem;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    pointer-events: none;
    backdrop-filter: blur(12px);
  }

  .overlay.error {
    color: #ffb199;
  }
  .hover-card {
    position: absolute;
    right: 1rem;
    bottom: 1rem;
    z-index: 1;
    display: grid;
    gap: 0.2rem;
    max-width: min(24rem, calc(100% - 2rem));
    border: 1px solid rgba(223, 245, 154, 0.14);
    border-radius: 10px;
    background: rgba(15, 18, 14, 0.9);
    box-shadow: 0 18px 46px rgba(0, 0, 0, 0.28);
    padding: 0.65rem 0.8rem;
    pointer-events: none;
    backdrop-filter: blur(12px);
  }

  .hover-card span {
    color: #dff59a;
    font-size: 0.68rem;
    font-weight: 800;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .hover-card strong {
    overflow: hidden;
    color: #fbf6eb;
    font-size: 0.9rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  canvas {
    display: block;
    width: 100%;
    height: 100%;
    cursor: pointer;
  }
</style>
