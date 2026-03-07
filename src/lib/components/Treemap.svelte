<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  interface LayoutRect {
    id: number;
    x: number;
    y: number;
    w: number;
    h: number;
  }

  let { rootId = 0, onNavigate } = $props<{ 
    rootId: number, 
    onNavigate: (id: number) => void 
  }>();

  let canvas: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D | null = null;
  let layout: LayoutRect[] = $state([]);
  let width = $state(800);
  let height = $state(600);
  let container: HTMLDivElement;
  let hoveredId = $state<number | null>(null);

  async function updateLayout() {
    if (!canvas) return;
    try {
      layout = await invoke("get_treemap_layout", {
        rootId,
        width,
        height,
      });
      render();
    } catch (e) {
      console.error("Failed to get layout", e);
    }
  }

  $effect(() => {
    // Re-run layout when rootId changes
    const _id = rootId;
    updateLayout();
  });

  function handleMouseMove(e: MouseEvent) {
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    hoveredId = null;
    for (const item of layout) {
      if (x >= item.x && x <= item.x + item.w && y >= item.y && y <= item.y + item.h) {
        hoveredId = item.id;
        break;
      }
    }
    render();
  }

  function handleClick() {
    if (hoveredId !== null) {
      onNavigate(hoveredId);
    }
  }

  function render() {
    if (!ctx) return;
    ctx.clearRect(0, 0, width, height);

    for (const rect of layout) {
      const isHovered = rect.id === hoveredId;
      const hue = (rect.id * 137.5) % 360;
      
      // Background
      ctx.fillStyle = `hsl(${hue}, 60%, ${isHovered ? 60 : 40}%)`;
      ctx.fillRect(rect.x, rect.y, rect.w, rect.h);

      // Cushion effect
      const gradient = ctx.createRadialGradient(
        rect.x + rect.w/2, rect.y + rect.h/2, 0,
        rect.x + rect.w/2, rect.y + rect.h/2, Math.max(rect.w, rect.h)
      );
      gradient.addColorStop(0, "rgba(255, 255, 255, 0.15)");
      gradient.addColorStop(1, "rgba(0, 0, 0, 0.3)");
      ctx.fillStyle = gradient;
      ctx.fillRect(rect.x, rect.y, rect.w, rect.h);

      // Border
      ctx.strokeStyle = isHovered ? "#fff" : "#000";
      ctx.lineWidth = isHovered ? 2 : 0.5;
      ctx.strokeRect(rect.x, rect.y, rect.w, rect.h);
    }
  }

  onMount(() => {
    ctx = canvas.getContext("2d");
    
    const resizeObserver = new ResizeObserver((entries) => {
      for (const entry of entries) {
        width = entry.contentRect.width;
        height = entry.contentRect.height;
        updateLayout();
      }
    });

    resizeObserver.observe(container);
    return () => resizeObserver.disconnect();
  });
</script>

<div class="treemap-container" bind:this={container}>
  <canvas 
    bind:this={canvas} 
    {width} 
    {height}
    onmousemove={handleMouseMove}
    onmouseleave={() => { hoveredId = null; render(); }}
    onclick={handleClick}
  ></canvas>
</div>

<style>
  .treemap-container {
    width: 100%;
    height: 100%;
    background: #1a1a1a;
    overflow: hidden;
    position: relative;
    border-radius: 8px;
    border: 1px solid #333;
  }

  canvas {
    display: block;
    image-rendering: pixelated;
  }
</style>
