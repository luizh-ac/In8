<script lang="ts">
  import AppCard from "../components/AppCard.svelte";
  import { store } from "../store.svelte";
  import { categoryLabels, categoryOrder, text } from "../strings.svelte";

  let query = $state("");
  let hideUnavailable = $state(true);

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return store.catalog.filter(
      (app) =>
        (!hideUnavailable ||
          app.resolution.status === "available" ||
          app.wslResolution?.status === "available") &&
        (q === "" || `${app.name} ${app.description} ${app.id}`.toLowerCase().includes(q)),
    );
  });

  const groups = $derived(
    categoryOrder
      .map((category) => ({ category, apps: filtered.filter((app) => app.category === category) }))
      .filter((group) => group.apps.length > 0),
  );
</script>

<div class="flex items-center gap-3">
  <h2 class="text-xl font-semibold">{text.apps.title}</h2>
  <input
    type="search"
    bind:value={query}
    placeholder={text.apps.searchPlaceholder}
    aria-label={text.apps.searchPlaceholder}
    class="ml-auto w-64 rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm outline-none focus:border-indigo-500 dark:border-zinc-700 dark:bg-zinc-900"
  />
  <label class="flex items-center gap-2 text-sm text-zinc-600 dark:text-zinc-400">
    <input type="checkbox" bind:checked={hideUnavailable} class="accent-indigo-500" />
    {text.apps.hideUnavailable}
  </label>
  <button
    type="button"
    onclick={() => store.refreshInstalled()}
    disabled={store.checkingInstalled}
    class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100 disabled:opacity-60 dark:border-zinc-700 dark:hover:bg-zinc-800"
  >
    {store.checkingInstalled ? text.apps.checkingInstalled : text.apps.checkInstalled}
  </button>
</div>

{#if store.platform && store.platform.managers.length === 0}
  <p class="mt-4 rounded-md bg-amber-50 p-3 text-sm text-amber-800 dark:bg-amber-950/40 dark:text-amber-300">
    {text.apps.noManagers}
  </p>
{/if}

{#if store.platform?.os === "windows" && !store.platform.managers.includes("winget")}
  <div class="mt-4 flex flex-wrap items-center gap-3 rounded-md bg-amber-50 p-3 text-sm text-amber-800 dark:bg-amber-950/40 dark:text-amber-300">
    <p class="flex-1">{store.repairWinget ? text.apps.wingetQueued : text.apps.noWinget}</p>
    <button
      type="button"
      aria-pressed={store.repairWinget}
      onclick={() => (store.repairWinget = !store.repairWinget)}
      class="rounded-md border px-3 py-1.5 text-sm {store.repairWinget
        ? 'border-indigo-500 bg-indigo-50 text-indigo-800 dark:bg-indigo-950/40 dark:text-indigo-200'
        : 'border-amber-400 hover:bg-amber-100 dark:border-amber-700 dark:hover:bg-amber-900/40'}"
    >
      {store.repairWinget ? "✓ " : ""}{text.apps.repairWinget}
    </button>
  </div>
{/if}

{#if store.platform?.os === "linux" && !store.platform.managers.includes("flatpak") && store.platform.managers.length > 0}
  <div class="mt-4 flex flex-wrap items-center gap-3 rounded-md bg-amber-50 p-3 text-sm text-amber-800 dark:bg-amber-950/40 dark:text-amber-300">
    <p class="flex-1">{store.setupFlatpak ? text.apps.flatpakQueued : text.apps.noFlatpak}</p>
    <button
      type="button"
      aria-pressed={store.setupFlatpak}
      onclick={() => (store.setupFlatpak = !store.setupFlatpak)}
      class="rounded-md border px-3 py-1.5 text-sm {store.setupFlatpak
        ? 'border-indigo-500 bg-indigo-50 text-indigo-800 dark:bg-indigo-950/40 dark:text-indigo-200'
        : 'border-amber-400 hover:bg-amber-100 dark:border-amber-700 dark:hover:bg-amber-900/40'}"
    >
      {store.setupFlatpak ? "✓ " : ""}{text.apps.installFlatpak}
    </button>
  </div>
{/if}

{#if groups.length === 0}
  <p class="mt-8 text-sm text-zinc-500">{text.apps.noResults}</p>
{/if}

{#each groups as group (group.category)}
  <section class="mt-6">
    <h3 class="mb-2 text-xs font-semibold tracking-wide text-zinc-500 uppercase">
      {categoryLabels[group.category]}
    </h3>
    <div class="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-2">
      {#each group.apps as app (app.id)}
        <AppCard {app} />
      {/each}
    </div>
  </section>
{/each}
