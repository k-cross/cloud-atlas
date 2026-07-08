<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { GraphController, type LegendCount } from '$lib/GraphController';

  let container: HTMLElement;
  let controller: GraphController | null = null;

  let statusText = $state('loading…');
  let legendCounts = $state<LegendCount[]>([]);
  let errorMsg = $state<string | null>(null);

  onMount(() => {
    controller = new GraphController({
      container,
      onStatusChange: (status) => {
        statusText = status;
      },
      onLegendChange: (legend) => {
        legendCounts = legend;
      },
      onError: (err) => {
        errorMsg = err;
      }
    });

    controller.initialize();
  });

  onDestroy(() => {
    if (controller) {
      controller.destroy();
    }
  });

  function handleReheat() {
    if (controller) {
      controller.layoutAndReveal(false);
    }
  }
</script>

<svelte:head>
  <title>cloud-atlas</title>
</svelte:head>

<div bind:this={container} class="graph-container"></div>

<div class="panel glass-panel">
  <h1>cloud-atlas</h1>
  <div class="status">{statusText}</div>
  
  <div class="legend">
    {#each legendCounts as { provider, color, count }}
      <div class="legend-row">
        <span class="swatch" style="background: {color}"></span>
        <span>{provider}</span>
        <span class="count">{count}</span>
      </div>
    {/each}
  </div>
  
  <button onclick={handleReheat}>Reheat layout</button>
</div>

{#if errorMsg}
  <div class="error-overlay">
    {errorMsg}
  </div>
{/if}

<style>
  .graph-container {
    position: absolute;
    inset: 0;
    z-index: 0;
  }

  .panel {
    position: absolute;
    top: 16px;
    left: 16px;
    z-index: 1;
    min-width: 200px;
    max-width: 280px;
    padding: 16px;
  }

  h1 {
    margin: 0 0 8px;
    font-size: 16px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .status {
    color: var(--muted);
    margin-bottom: 12px;
    font-size: 12px;
  }

  .legend {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 16px;
  }

  .legend-row {
    display: flex;
    align-items: center;
    font-size: 13px;
  }

  .swatch {
    display: inline-block;
    width: 10px;
    height: 10px;
    margin-right: 8px;
    border-radius: 50%;
  }

  .count {
    color: var(--muted);
    margin-left: 4px;
  }

  .error-overlay {
    position: absolute;
    inset: 0;
    z-index: 2;
    display: grid;
    place-content: center;
    padding: 24px;
    text-align: center;
    white-space: pre-wrap;
    color: #ff7a7a;
    background: rgba(15, 18, 22, 0.9);
    backdrop-filter: blur(8px);
    font-size: 14px;
  }
</style>
