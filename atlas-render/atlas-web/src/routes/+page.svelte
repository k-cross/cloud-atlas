<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { GraphController, type LegendCount } from '$lib/GraphController';
  import { CONFIRMED, INFERRED, type ServiceSummary } from '$lib/service';
  import { FLOW_STATUS_COLORS, SERVES_STATUS_COLORS } from '$lib/style';
  import { formatAge, formatBytes, type TrafficSummary } from '$lib/traffic';

  let container: HTMLElement;
  let controller: GraphController | null = null;

  let statusText = $state('loading…');
  let legendCounts = $state<LegendCount[]>([]);
  let errorMsg = $state<string | null>(null);
  let traffic = $state<TrafficSummary | null>(null);
  let service = $state<ServiceSummary | null>(null);
  let serviceView = $state(false);
  let derivedEdges = $derived((service?.confirmed ?? 0) + (service?.inferred ?? 0));
  let now = $state(Date.now());

  const statusOrder = ['accepted', 'rejected', 'mixed', 'observed'];
  let flowStatuses = $derived(
    statusOrder
      .filter((s) => (traffic?.byStatus[s] ?? 0) > 0)
      .map((s) => ({ status: s, count: traffic!.byStatus[s], color: FLOW_STATUS_COLORS[s] }))
  );

  onMount(() => {
    controller = new GraphController({
      container,
      onStatusChange: (status) => {
        statusText = status;
      },
      onLegendChange: (legend) => {
        legendCounts = legend;
      },
      onTrafficChange: (summary) => {
        traffic = summary;
        now = Date.now();
      },
      onServiceChange: (summary) => {
        service = summary;
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

  function handleServiceView(event: Event) {
    serviceView = (event.currentTarget as HTMLInputElement).checked;
    controller?.setServiceView(serviceView);
  }

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

  {#if traffic && traffic.flows > 0}
    <div class="traffic">
      <h2>Observed traffic</h2>
      <div class="traffic-row">
        <span>{traffic.flows} flow{traffic.flows === 1 ? '' : 's'}</span>
        <span class="muted">{traffic.liveNodes} live</span>
      </div>
      <div class="traffic-row">
        <span>{traffic.packets.toLocaleString()} packets</span>
        <span class="muted">{formatBytes(traffic.bytes)}</span>
      </div>
      <div class="statuses">
        {#each flowStatuses as { status, count, color }}
          <span class="chip" style="--chip: {color}">{status} {count}</span>
        {/each}
      </div>
      <div class="muted">last seen {formatAge(now - traffic.newest)}</div>
    </div>
  {/if}

  <!-- Stays while the view is on, even with nothing derived: a patch that
       removes the last Serves edge must not take the only way back with it. -->
  {#if serviceView || derivedEdges > 0}
    <div class="service">
      <h2>Service topology</h2>
      <div class="statuses">
        <span class="chip" style="--chip: {SERVES_STATUS_COLORS[CONFIRMED]}"
          >{CONFIRMED} {service?.confirmed ?? 0}</span
        >
        <span class="chip" style="--chip: {SERVES_STATUS_COLORS[INFERRED]}"
          >{INFERRED} {service?.inferred ?? 0}</span
        >
      </div>
      <label class="toggle">
        <input type="checkbox" checked={serviceView} onchange={handleServiceView} />
        <span>Service view</span>
      </label>
      <div class="muted">
        {#if serviceView && derivedEdges === 0}
          nothing derived yet — the view is empty
        {:else if serviceView}
          plumbing hidden
        {:else}
          wired vs. used
        {/if}
      </div>
    </div>
  {/if}

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

  .muted {
    color: var(--muted);
  }

  .traffic,
  .service {
    margin-bottom: 16px;
    padding-top: 12px;
    border-top: 1px solid rgba(255, 255, 255, 0.08);
  }

  h2 {
    margin: 0 0 8px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--muted);
  }

  .traffic-row {
    display: flex;
    justify-content: space-between;
    font-size: 13px;
    margin-bottom: 4px;
  }

  .statuses {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin: 8px 0 6px;
  }

  .toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    margin-bottom: 4px;
    cursor: pointer;
  }

  .chip {
    font-size: 11px;
    padding: 2px 6px;
    border-radius: 10px;
    color: var(--chip);
    border: 1px solid color-mix(in srgb, var(--chip) 45%, transparent);
    background: color-mix(in srgb, var(--chip) 12%, transparent);
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
