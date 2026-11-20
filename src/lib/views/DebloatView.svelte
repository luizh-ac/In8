<script lang="ts">
  import { store } from "../store.svelte";
  import { text } from "../strings";
  import type { DebloatInfo } from "../types";

  const t = text.debloat;

  const installed = $derived(store.debloatItems.filter((item) => item.installed));
  const safe = $derived(installed.filter((item) => item.level === "safe"));
  const advanced = $derived(installed.filter((item) => item.level === "advanced"));
  const missing = $derived(store.debloatItems.length - installed.length);

  function selectAllSafe(): void {
    for (const item of safe) store.debloatSelected.add(item.id);
  }
</script>

{#snippet itemList(items: DebloatInfo[])}
  <ul class="mt-3 divide-y divide-zinc-200 rounded-lg border border-zinc-200 dark:divide-zinc-800 dark:border-zinc-800">
    {#each items as item (item.id)}
      <li>
        <label class="flex cursor-pointer items-start gap-3 px-4 py-3">
          <input
            type="checkbox"
            checked={store.debloatSelected.has(item.id)}
            onchange={() => store.toggleDebloat(item.id)}
            class="mt-1 accent-indigo-500"
          />
          <span class="min-w-0">
            <span class="block text-sm font-medium">{item.name}</span>
            <span class="block text-sm text-zinc-500">{item.description}</span>
          </span>
        </label>
      </li>
    {/each}
  </ul>
{/snippet}

<div class="flex items-center gap-3">
  <h2 class="text-xl font-semibold">{t.title}</h2>
  <button
    type="button"
    disabled={store.checkingDebloat}
    onclick={() => store.refreshDebloat()}
    class="ml-auto rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100 disabled:opacity-50 dark:border-zinc-700 dark:hover:bg-zinc-800"
  >
    {store.checkingDebloat ? t.checking : t.checkAgain}
  </button>
</div>
<p class="mt-1 text-sm text-zinc-500">{t.intro}</p>

{#if store.debloatError}
  <p class="mt-4 rounded-md bg-red-50 p-3 text-sm text-red-700 dark:bg-red-950/40 dark:text-red-300">
    {store.debloatError}
  </p>
{:else if store.checkingDebloat && store.debloatItems.length === 0}
  <p class="mt-6 text-sm text-zinc-500">{t.checking}</p>
{:else if store.debloatItems.length > 0 && installed.length === 0}
  <p class="mt-6 text-sm text-zinc-500">{t.nothing}</p>
{/if}

{#if safe.length > 0}
  <section class="mt-8">
    <div class="flex items-center gap-3">
      <h3 class="text-sm font-semibold tracking-wide text-zinc-500 uppercase">{t.safe}</h3>
      <button type="button" onclick={selectAllSafe} class="text-xs text-indigo-600 hover:text-indigo-500 dark:text-indigo-400">
        {t.selectSafe}
      </button>
    </div>
    {@render itemList(safe)}
  </section>
{/if}

{#if advanced.length > 0}
  <section class="mt-8">
    <h3 class="text-sm font-semibold tracking-wide text-amber-600 uppercase dark:text-amber-400">{t.advanced}</h3>
    <p class="mt-1 text-xs text-zinc-500">{t.advancedHint}</p>
    {@render itemList(advanced)}
  </section>
{/if}

{#if missing > 0 && installed.length > 0}
  <p class="mt-6 text-xs text-zinc-400">{t.notInstalled(missing)}</p>
{/if}
