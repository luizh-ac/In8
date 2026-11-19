<script lang="ts">
  import type { CatalogApp, Target } from "../types";
  import { store } from "../store.svelte";
  import { sourceLabel, text } from "../strings";

  let { app }: { app: CatalogApp } = $props();

  const target = $derived(store.targetOf(app));
  const resolution = $derived(store.resolutionFor(app));
  const available = $derived(resolution?.status === "available");
  const selected = $derived(store.selected.has(app.id));
  const installed = $derived(store.isInstalled(app));
  const source = $derived(sourceLabel(app, target));
  const targets: Target[] = ["host", "wsl"];
</script>

<div
  class="flex flex-col rounded-lg border transition-colors
    {selected
    ? 'border-indigo-500 bg-indigo-50 dark:border-indigo-400 dark:bg-indigo-950/40'
    : 'border-zinc-200 bg-white hover:border-zinc-300 dark:border-zinc-800 dark:bg-zinc-900 dark:hover:border-zinc-700'}"
>
  <button
    type="button"
    role="checkbox"
    aria-checked={selected}
    disabled={!available}
    title={resolution?.status === "unavailable" ? resolution.reason : undefined}
    onclick={() => store.toggle(app)}
    class="flex w-full items-start gap-3 p-3 text-left disabled:cursor-not-allowed disabled:opacity-50"
  >
    <span
      class="mt-0.5 flex size-4 shrink-0 items-center justify-center rounded border text-[10px] leading-none
        {selected ? 'border-indigo-500 bg-indigo-500 text-white' : 'border-zinc-300 dark:border-zinc-600'}"
      aria-hidden="true"
    >
      {selected ? "✓" : ""}
    </span>
    <span class="min-w-0 flex-1">
      <span class="flex items-center gap-2">
        <span class="truncate text-sm font-medium">{app.name}</span>
        {#if installed}
          <span
            class="shrink-0 rounded bg-emerald-100 px-1.5 py-0.5 text-[11px] font-medium text-emerald-800 dark:bg-emerald-900/50 dark:text-emerald-300"
          >
            {text.apps.installed}
          </span>
        {/if}
      </span>
      <span class="block truncate text-xs text-zinc-500">
        {resolution?.status === "unavailable" ? resolution.reason : app.description}
      </span>
      {#if source}
        <span class="mt-1 block text-[11px] text-zinc-400">{source}</span>
      {/if}
    </span>
  </button>

  {#if app.wslResolution}
    <div class="flex gap-1 px-3 pb-3" role="radiogroup" aria-label="Install target for {app.name}">
      {#each targets as option (option)}
        <button
          type="button"
          role="radio"
          aria-checked={target === option}
          onclick={() => store.setTarget(app, option)}
          class="rounded px-2 py-0.5 text-[11px] font-medium transition-colors
            {target === option
            ? 'bg-zinc-800 text-white dark:bg-zinc-200 dark:text-zinc-900'
            : 'text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800'}"
        >
          {text.targets[option]}
        </button>
      {/each}
    </div>
  {/if}
</div>
