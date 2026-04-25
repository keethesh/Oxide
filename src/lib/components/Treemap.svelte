<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { TreemapRect } from "$lib/types";

  const LABEL_MIN_WIDTH = 72;
  const LABEL_MIN_HEIGHT = 22;
  const HOVER_BORDER = "#fff7eb";

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

  function render() {
    if (!ctx) {
      return;
    }

    ctx.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
    ctx.clearRect(0, 0, width, height);

    for (const rect of layout) {
      const isHovered = sameRect(rect, hoveredRect);
      const fill = rect.kind === "overflow"
        ? isHovered
          ? "#6a554d"
          : "#4d3a34"
        : `hsl(${((rect.id ?? 0) * 137.5) % 360}, 54%, ${isHovered ? 56 : 42}%)`;

      ctx.fillStyle = fill;
      ctx.fillRect(rect.x, rect.y, rect.w, rect.h);

      ctx.strokeStyle = isHovered ? HOVER_BORDER : "rgba(17, 17, 17, 0.45)";
      ctx.lineWidth = isHovered ? 2 : 1;
      ctx.strokeRect(rect.x, rect.y, rect.w, rect.h);

      if (rect.w >= LABEL_MIN_WIDTH && rect.h >= LABEL_MIN_HEIGHT) {
        ctx.save();
        ctx.beginPath();
        ctx.rect(rect.x + 2, rect.y + 2, rect.w - 4, rect.h - 4);
        ctx.clip();
        ctx.fillStyle = "rgba(255, 250, 244, 0.92)";
        ctx.font = "12px 'Segoe UI', sans-serif";
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
  {:else if layout.some((rect) => rect.kind === "overflow")}
    <div class="overlay subtle">Small tiles grouped into Other</div>
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
    background: #101210;
    overflow: hidden;
    position: relative;
    border: 1px solid rgba(236, 232, 223, 0.08);
  }

  .overlay {
    position: absolute;
    top: 1rem;
    left: 1rem;
    z-index: 1;
    border-radius: 4px;
    background: rgba(10, 10, 10, 0.72);
    padding: 0.45rem 0.8rem;
    color: #f6f2e9;
    font-size: 0.82rem;
  }

  .overlay.error {
    color: #ffb199;
  }

  .overlay.subtle {
    color: #a8a094;
  }

  canvas {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
