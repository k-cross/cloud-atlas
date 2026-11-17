<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { GraphController, type LegendCount } from '$lib/GraphController';
  import { type CollectionHealth, healthLevel, healthRows } from '$lib/health';
  import { emptySearch, excerpt, type SearchHit, type SearchResult, searchTokens } from '$lib/search';
  import { CONFIRMED, INFERRED, type ServiceSummary } from '$lib/service';
  import {
    FLOW_STATUS_COLORS,
    HEALTH_COLORS,
    PROVIDER_COLORS,
    providerOf,
    SERVES_STATUS_COLORS
  } from '$lib/style';
  import { formatAge, formatBytes, type TrafficSummary } from '$lib/traffic';

  const SHOWN_FAILURES = 2;

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

  let health = $state<CollectionHealth | null>(null);
  let healthList = $derived(health ? healthRows(health) : []);
  let overallHealth = $derived(healthLevel(healthList));

  let searchInput: HTMLInputElement;
  let query = $state('');
  let search = $state<SearchResult>(emptySearch());
  let resultsOpen = $state(false);
  let active = $state(0);
  let focusedLabel = $state<string | null>(null);
  let tokens = $derived(searchTokens(query));

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
      onHealthChange: (latest) => {
        health = latest;
      },
      onSearchChange: (result) => {
        search = result;
        active = Math.min(active, Math.max(result.hits.length - 1, 0));
      },
      onFocusChange: (_key, label) => {
        focusedLabel = label;
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

  function handleSearchInput(event: Event) {
    query = (event.currentTarget as HTMLInputElement).value;
    active = 0;
    resultsOpen = true;
    if (focusedLabel) controller?.focus(null);
    controller?.setQuery(query);
  }

  function choose(hit: SearchHit | undefined) {
    if (!hit) return;
    resultsOpen = false;
    controller?.focus(hit.key);
  }

  function clearSearch() {
    query = '';
    resultsOpen = false;
    controller?.setQuery('');
    controller?.focus(null);
  }

  function handleSearchKey(event: KeyboardEvent) {
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        resultsOpen = true;
        active = Math.min(active + 1, Math.max(search.hits.length - 1, 0));
        break;
      case 'ArrowUp':
        event.preventDefault();
        active = Math.max(active - 1, 0);
        break;
      case 'Enter':
        event.preventDefault();
        choose(search.hits[active]);
        break;
      case 'Escape':
        event.preventDefault();
        clearSearch();
        searchInput.blur();
        break;
    }
  }

  function handleWindowKey(event: KeyboardEvent) {
    const target = event.target as HTMLElement | null;
    const typing = target?.closest('input, textarea, [contenteditable="true"]');
    if (event.key === '/' && !typing) {
      event.preventDefault();
      searchInput.focus();
      searchInput.select();
    } else if (event.key === 'Escape' && !typing && (focusedLabel || query)) {
      clearSearch();
    }
  }

  function matchSummary(result: SearchResult): string {
    if (result.total === 0) return 'no matches';
    if (result.total > result.hits.length) return `${result.hits.length} of ${result.total} matches`;
    return `${result.total} match${result.total === 1 ? '' : 'es'}`;
  }
</script>

<svelte:window onkeydown={handleWindowKey} />

<svelte:head>
  <title>cloud-atlas</title>
</svelte:head>

<div bind:this={container} class="graph-container"></div>

<div class="panel glass-panel">
  <h1>cloud-atlas</h1>

  <div class="search">
    <input
      bind:this={searchInput}
      type="search"
      placeholder="Search nodes"
      aria-label="Search nodes"
      role="combobox"
      aria-expanded={resultsOpen && tokens.length > 0}
      aria-controls="search-results"
      aria-activedescendant={resultsOpen && search.hits[active] ? `hit-${active}` : undefined}
      autocomplete="off"
      spellcheck="false"
      value={query}
      oninput={handleSearchInput}
      onkeydown={handleSearchKey}
    />
    {#if !query}<kbd>/</kbd>{/if}
  </div>

  {#if resultsOpen && tokens.length > 0}
    <div class="results" id="search-results" role="listbox">
      {#each search.hits as hit, i (hit.key)}
        <button
          id="hit-{i}"
          class="result"
          class:active={i === active}
          role="option"
          aria-selected={i === active}
          title={hit.label}
          onclick={() => choose(hit)}
          onmouseenter={() => (active = i)}
        >
          <span class="swatch" style="background: {PROVIDER_COLORS[providerOf(hit.kind)]}"></span>
          <span class="result-text">
            <span class="result-label"
              >{#each excerpt(hit.label, tokens) as segment}{#if segment.match}<mark
                    >{segment.text}</mark
                  >{:else}{segment.text}{/if}{/each}</span
            >
            <span class="result-kind">{hit.kind}</span>
          </span>
        </button>
      {/each}
      <div class="muted results-count">{matchSummary(search)}</div>
    </div>
  {/if}

  {#if focusedLabel}
    <div class="focus-chip" title={focusedLabel}>
      <div class="focus-head">
        <span class="muted">focused</span>
        <button class="clear" aria-label="Clear focus" onclick={clearSearch}>×</button>
      </div>
      <div class="focus-label">
        {#each excerpt(focusedLabel, []) as segment}{segment.text}{/each}
      </div>
    </div>
  {/if}

  <div class="status">{statusText}</div>

  {#if health}
    <div class="health" data-level={overallHealth}>
      <h2>Collection</h2>
      {#if healthList.length === 0}
        <div class="health-row">
          <span class="dot" style="--dot: {HEALTH_COLORS.ok}"></span>
          <span>all sources read</span>
        </div>
      {:else}
        {#each healthList as row (row.subject)}
          <div class="health-row" data-level={row.level}>
            <span class="dot" style="--dot: {HEALTH_COLORS[row.level]}"></span>
            <span class="subject">{row.subject}</span>
            <span class="muted detail">{row.detail}</span>
          </div>
          {#each row.failures.slice(0, SHOWN_FAILURES) as failure}
            <div class="failure muted" title={failure}>{failure}</div>
          {/each}
          {#if row.failures.length > SHOWN_FAILURES}
            <div class="failure muted">+{row.failures.length - SHOWN_FAILURES} more</div>
          {/if}
        {/each}
      {/if}
    </div>
  {/if}

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
    box-sizing: border-box;
    width: 280px;
    max-width: calc(100vw - 32px);
    max-height: calc(100vh - 32px);
    overflow-y: auto;
    padding: 16px;
  }

  .search {
    position: relative;
    margin-bottom: 8px;
  }

  .search input {
    box-sizing: border-box;
    width: 100%;
    padding: 6px 28px 6px 10px;
    font: inherit;
    font-size: 13px;
    color: var(--text);
    background: rgba(15, 18, 22, 0.6);
    border: 1px solid #37404d;
    border-radius: 6px;
    outline: none;
  }

  .search input:focus {
    border-color: var(--primary);
  }

  .search kbd {
    position: absolute;
    top: 50%;
    right: 8px;
    transform: translateY(-50%);
    padding: 0 5px;
    font-family: inherit;
    font-size: 11px;
    color: var(--muted);
    border: 1px solid #37404d;
    border-radius: 4px;
    pointer-events: none;
  }

  .results {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 8px;
  }

  .result {
    display: flex;
    align-items: flex-start;
    gap: 0;
    width: 100%;
    padding: 4px 6px;
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 6px;
    font-weight: 400;
  }

  .result .swatch {
    flex: none;
    margin-top: 5px;
  }

  .result.active {
    background: rgba(66, 133, 244, 0.14);
    border-color: rgba(66, 133, 244, 0.35);
  }

  .result-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .result-label,
  .focus-label,
  .failure {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .result-label {
    font-size: 12px;
  }

  .result-label mark {
    color: inherit;
    font-weight: 600;
    background: rgba(66, 133, 244, 0.3);
    border-radius: 2px;
  }

  .result-kind,
  .results-count {
    font-size: 11px;
    color: var(--muted);
  }

  .results-count {
    padding: 2px 6px 0;
  }

  .focus-chip {
    margin-bottom: 8px;
    padding: 6px 8px;
    font-size: 12px;
    border: 1px solid rgba(66, 133, 244, 0.35);
    border-radius: 6px;
    background: rgba(66, 133, 244, 0.08);
  }

  .focus-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 11px;
  }

  .clear {
    flex: none;
    padding: 0 6px;
    line-height: 18px;
  }

  .health-row {
    display: flex;
    align-items: baseline;
    gap: 6px;
    font-size: 13px;
  }

  .health-row .detail {
    font-size: 12px;
  }

  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--dot);
    box-shadow: 0 0 6px color-mix(in srgb, var(--dot) 60%, transparent);
  }

  .failure {
    padding-left: 14px;
    font-size: 11px;
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
  .service,
  .health {
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
