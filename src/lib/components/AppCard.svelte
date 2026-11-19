<script lang="ts">
  import type { CatalogApp } from "../types";
  import { sourceLabel, text } from "../strings";

  let {
    app,
    selected,
    installed,
    onToggle,
  }: { app: CatalogApp; selected: boolean; installed: boolean; onToggle: () => void } = $props();

  const available = $derived(app.resolution.status === "available");
  const source = $derived(sourceLabel(app));
</script>

<button
  type="button"
  role="checkbox"
  aria-checked={selected}
  disabled={!available}
  title={app.resolution.status === "unavailable" ? app.resolution.reason : undefined}
  onclick={onToggle}
  class="flex w-full items-start gap-3 rounded-lg border p-3 text-left transition-colors
    {selected
    ? 'border-indigo-500 bg-indigo-50 dark:border-indigo-400 dark:bg-indigo-950/40'
    : 'border-zinc-200 bg-white hover:border-zinc-300 dark:border-zinc-800 dark:bg-zinc-900 dark:hover:border-zinc-700'}
    disabled:cursor-not-allowed disabled:opacity-50"
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
      {app.resolution.status === "available" ? app.description : app.resolution.reason}
    </span>
    {#if source}
      <span class="mt-1 block text-[11px] text-zinc-400">{source}</span>
    {/if}
  </span>
</button>
