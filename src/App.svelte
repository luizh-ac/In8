<script lang="ts">
  import Sidebar from "./lib/components/Sidebar.svelte";
  import AppsView from "./lib/views/AppsView.svelte";
  import DebloatView from "./lib/views/DebloatView.svelte";
  import PostInstallView from "./lib/views/PostInstallView.svelte";
  import ProfilesView from "./lib/views/ProfilesView.svelte";
  import RunView from "./lib/views/RunView.svelte";
  import SettingsView from "./lib/views/SettingsView.svelte";
  import TweaksView from "./lib/views/TweaksView.svelte";
  import WslView from "./lib/views/WslView.svelte";
  import { store } from "./lib/store.svelte";
  import { text } from "./lib/strings.svelte";
  import { onMount } from "svelte";

  onMount(() => {
    store.load();
  });

  const placeholderTitle = $derived(
    store.view === "apps" || store.view === "run" ? "" : text.nav[store.view],
  );
  const showBar = $derived(
    ["apps", "profiles", "wsl", "tweaks", "debloat", "postinstall"].includes(store.view) &&
      store.selectionCount > 0,
  );

  function review(): void {
    store.previousView = store.view;
    store.view = "run";
  }
</script>

<div class="flex h-full bg-zinc-50 text-zinc-900 dark:bg-zinc-950 dark:text-zinc-100">
  <Sidebar />

  <div class="flex min-w-0 flex-1 flex-col">
    {#if store.update}
      <div class="flex items-center gap-3 border-b border-indigo-200 bg-indigo-50 px-8 py-2 text-sm text-indigo-800 dark:border-indigo-900 dark:bg-indigo-950/40 dark:text-indigo-200">
        <span>{text.update.available(store.update.version)}</span>
        {#if store.updateError}
          <span class="text-red-600 dark:text-red-400">{store.updateError}</span>
        {/if}
        <button
          type="button"
          disabled={store.installingUpdate || store.running}
          onclick={() => store.installUpdate()}
          class="ml-auto rounded-md bg-indigo-600 px-3 py-1 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
        >
          {store.installingUpdate ? text.update.installing : text.update.install}
        </button>
      </div>
    {/if}
    <main class="flex-1 overflow-y-auto p-8">
      {#if store.loadError}
        <p class="rounded-md bg-red-50 p-3 text-sm text-red-700 dark:bg-red-950/40 dark:text-red-300">
          {store.loadError}
        </p>
      {:else if store.view === "apps"}
        <AppsView />
      {:else if store.view === "profiles"}
        <ProfilesView />
      {:else if store.view === "wsl"}
        <WslView />
      {:else if store.view === "tweaks"}
        <TweaksView />
      {:else if store.view === "debloat"}
        <DebloatView />
      {:else if store.view === "postinstall"}
        <PostInstallView />
      {:else if store.view === "settings"}
        <SettingsView />
      {:else if store.view === "run"}
        <RunView />
      {:else}
        <h2 class="text-xl font-semibold">{placeholderTitle}</h2>
        <p class="mt-2 text-sm text-zinc-500">{text.comingSoon}</p>
      {/if}
    </main>

    {#if showBar}
      <footer
        class="flex items-center gap-4 border-t border-zinc-200 bg-white px-8 py-3 dark:border-zinc-800 dark:bg-zinc-900"
      >
        <span class="text-sm">{text.bar.selected(store.selectionCount)}</span>
        <button
          type="button"
          onclick={() => store.clearSelection()}
          class="text-sm text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200"
        >
          {text.bar.clear}
        </button>
        <button
          type="button"
          onclick={review}
          class="ml-auto rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500"
        >
          {text.bar.review}
        </button>
      </footer>
    {/if}
  </div>
</div>
