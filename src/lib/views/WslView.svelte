<script lang="ts">
  import { onMount } from "svelte";
  import {
    getOnlineDistros,
    getWslConfig,
    openDistroTerminal,
    previewWslConfig,
    saveWslConfig,
  } from "../api";
  import { message, store, type WslToggle } from "../store.svelte";
  import { text } from "../strings";
  import type { OnlineDistro, WslConfig } from "../types";

  const status = $derived(store.wsl);
  const active = $derived(status?.active ?? null);
  const ready = $derived(!!active && active.hasUser && active.apt);

  let online = $state<OnlineDistro[] | null>(null);
  let loadingOnline = $state(false);
  let pickedDistro = $state("");
  let actionError = $state<string | null>(null);

  let config = $state<WslConfig>({ memory: null, processors: null, swap: null });
  let configPreview = $state<string | null>(null);
  let configMessage = $state<string | null>(null);
  let configError = $state<string | null>(null);

  const vscodeSelected = $derived(
    store.selected.has("vscode") && store.targets.get("vscode") !== "wsl",
  );
  const systemdComing = $derived(store.wslToggles.has("enableSystemd"));

  onMount(async () => {
    if (!store.wsl) await store.refreshWsl();
    try {
      config = await getWslConfig();
    } catch (error) {
      configError = message(error);
    }
  });

  async function loadOnline(): Promise<void> {
    loadingOnline = true;
    actionError = null;
    try {
      online = await getOnlineDistros();
      pickedDistro = online[0]?.name ?? "";
    } catch (error) {
      actionError = message(error);
    } finally {
      loadingOnline = false;
    }
  }

  async function openDistro(name: string): Promise<void> {
    actionError = null;
    try {
      await openDistroTerminal(name);
    } catch (error) {
      actionError = message(error);
    }
  }

  function cleaned(): WslConfig {
    return {
      memory: config.memory?.trim() || null,
      processors: config.processors || null,
      swap: config.swap?.trim() || null,
    };
  }

  async function preview(): Promise<void> {
    configError = null;
    configMessage = null;
    try {
      configPreview = await previewWslConfig(cleaned());
    } catch (error) {
      configPreview = null;
      configError = message(error);
    }
  }

  async function save(): Promise<void> {
    configError = null;
    try {
      await saveWslConfig(cleaned());
      configPreview = null;
      configMessage = text.wsl.saved;
    } catch (error) {
      configError = message(error);
    }
  }
</script>

{#snippet option(kind: WslToggle, title: string, hint: string, done: boolean, blocked: string | null)}
  <label
    class="flex items-start gap-3 rounded-lg border border-zinc-200 bg-white p-3 dark:border-zinc-800 dark:bg-zinc-900
      {done || blocked ? 'opacity-70' : 'cursor-pointer'}"
  >
    {#if done}
      <span class="mt-0.5 text-xs font-medium text-emerald-600 dark:text-emerald-400">{text.wsl.done}</span>
    {:else}
      <input
        type="checkbox"
        class="mt-1 accent-indigo-500"
        checked={store.wslToggles.has(kind)}
        disabled={!!blocked}
        onchange={() => store.toggleWsl(kind)}
      />
    {/if}
    <span>
      <span class="block text-sm font-medium">{title}</span>
      <span class="block text-xs text-zinc-500">{blocked ?? hint}</span>
    </span>
  </label>
{/snippet}

<div class="flex items-center gap-3">
  <h2 class="text-xl font-semibold">{text.wsl.title}</h2>
  <button
    type="button"
    onclick={() => store.refreshWsl()}
    disabled={store.checkingWsl}
    class="ml-auto rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100 disabled:opacity-60 dark:border-zinc-700 dark:hover:bg-zinc-800"
  >
    {store.checkingWsl ? text.wsl.checking : text.wsl.checkAgain}
  </button>
</div>
<p class="mt-1 max-w-2xl text-sm text-zinc-500">{text.wsl.intro}</p>

{#if store.wslError || actionError}
  <p class="mt-4 rounded-md bg-red-50 p-3 text-sm text-red-700 dark:bg-red-950/40 dark:text-red-300">
    {store.wslError ?? actionError}
  </p>
{/if}

{#if !status}
  <p class="mt-6 text-sm text-zinc-500">{store.checkingWsl ? text.wsl.checking : ""}</p>
{:else if !status.installed}
  <p class="mt-6 text-sm">{text.wsl.notInstalled}</p>
  <div class="mt-3 max-w-xl">
    {@render option("installWsl", text.wsl.installWsl, text.wsl.installWslHint, false, null)}
  </div>
{:else}
  <!-- Distro -->
  <section class="mt-6 max-w-3xl rounded-lg border border-zinc-200 p-4 dark:border-zinc-800">
    <div class="flex flex-wrap items-center gap-3">
      <span class="text-xs text-zinc-500">{status.version ? text.wsl.version(status.version) : ""}</span>
      {#if status.distros.length > 1 && active}
        <label class="ml-auto flex items-center gap-2 text-sm">
          {text.wsl.distro}
          <select
            value={active.name}
            onchange={(event) => store.refreshWsl(event.currentTarget.value)}
            class="rounded-md border border-zinc-300 bg-white px-2 py-1 text-sm dark:border-zinc-700 dark:bg-zinc-900"
          >
            {#each status.distros as distro (distro.name)}
              <option value={distro.name}>{distro.name}{distro.isDefault ? " (default)" : ""}</option>
            {/each}
          </select>
        </label>
      {/if}
    </div>

    {#if active}
      <h3 class="mt-2 text-lg font-medium">{active.osName || active.name}</h3>
      {#if !active.hasUser}
        <p class="mt-2 text-sm text-amber-700 dark:text-amber-300">{text.wsl.createUser(active.name)}</p>
        <button
          type="button"
          onclick={() => openDistro(active.name)}
          class="mt-2 rounded-md bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-500"
        >
          {text.wsl.openDistro(active.name)}
        </button>
      {:else if !active.apt}
        <p class="mt-2 text-sm text-amber-700 dark:text-amber-300">{text.wsl.notApt}</p>
      {:else}
        <dl class="mt-2 grid grid-cols-[auto_1fr] gap-x-6 gap-y-1 text-sm">
          <dt class="text-zinc-500">{text.wsl.details.user}</dt>
          <dd>{active.user}</dd>
          <dt class="text-zinc-500">{text.wsl.details.systemd}</dt>
          <dd>{active.systemd ? text.wsl.details.on : text.wsl.details.off}</dd>
          <dt class="text-zinc-500">{text.wsl.details.docker}</dt>
          <dd>{active.docker ? text.wsl.details.installed : text.wsl.details.notInstalled}</dd>
        </dl>
      {/if}
    {:else}
      <p class="mt-2 text-sm">{text.wsl.noDistro}</p>
    {/if}

    <div class="mt-4 flex flex-wrap items-center gap-2 border-t border-zinc-200 pt-3 dark:border-zinc-800">
      <span class="text-sm font-medium">{text.wsl.installDistro}</span>
      {#if online === null}
        <button
          type="button"
          onclick={loadOnline}
          disabled={loadingOnline}
          class="rounded-md border border-zinc-300 px-3 py-1 text-sm hover:bg-zinc-100 disabled:opacity-60 dark:border-zinc-700 dark:hover:bg-zinc-800"
        >
          {loadingOnline ? text.wsl.loadingDistros : text.wsl.loadDistros}
        </button>
      {:else}
        <select
          bind:value={pickedDistro}
          aria-label={text.wsl.pickDistro}
          class="rounded-md border border-zinc-300 bg-white px-2 py-1 text-sm dark:border-zinc-700 dark:bg-zinc-900"
        >
          {#each online as distro (distro.name)}
            <option value={distro.name}>{distro.friendlyName}</option>
          {/each}
        </select>
        <button
          type="button"
          onclick={() => (store.distroToInstall = pickedDistro || null)}
          disabled={!pickedDistro}
          class="rounded-md border border-zinc-300 px-3 py-1 text-sm hover:bg-zinc-100 disabled:opacity-60 dark:border-zinc-700 dark:hover:bg-zinc-800"
        >
          {text.wsl.installDistro}
        </button>
      {/if}
      {#if store.distroToInstall}
        <span class="text-xs text-indigo-600 dark:text-indigo-400">
          {text.wsl.distroQueued(store.distroToInstall)}
        </span>
      {/if}
    </div>
  </section>

  {#if ready && active}
    <!-- Environment -->
    <section class="mt-6 max-w-3xl">
      <h3 class="mb-2 text-xs font-semibold tracking-wide text-zinc-500 uppercase">{text.wsl.environment}</h3>
      <div class="flex flex-col gap-2">
        {@render option("enableSystemd", text.wsl.systemd, text.wsl.systemdHint, active.systemd, null)}
        {@render option(
          "dockerEngine",
          text.wsl.docker,
          text.wsl.dockerHint,
          active.docker,
          !active.systemd && !systemdComing ? text.wsl.dockerNeedsSystemd : null,
        )}
        {#if status.dockerDesktop && !active.docker}
          <p class="rounded-md bg-amber-50 p-2 text-xs text-amber-800 dark:bg-amber-950/40 dark:text-amber-300">
            {text.wsl.dockerDesktop}
          </p>
        {/if}
        {@render option(
          "vscodeExtension",
          text.wsl.vscode,
          text.wsl.vscodeHint,
          status.vscodeExtension === true,
          status.vscodeExtension === null && !vscodeSelected ? text.wsl.vscodeMissing : null,
        )}
      </div>
    </section>

    <!-- Dev tools -->
    <section class="mt-6 max-w-3xl">
      <h3 class="mb-2 text-xs font-semibold tracking-wide text-zinc-500 uppercase">{text.wsl.devTools}</h3>
      <p class="text-sm text-zinc-500">{text.wsl.devToolsHint}</p>
      <button
        type="button"
        onclick={() => store.selectDevOnWsl()}
        class="mt-2 rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
      >
        {text.wsl.devToolsButton}
      </button>
      <p class="mt-2 text-xs text-zinc-500">{text.wsl.projectsTip}</p>
    </section>
  {/if}

  <!-- .wslconfig -->
  <section class="mt-6 max-w-3xl">
    <h3 class="mb-1 text-xs font-semibold tracking-wide text-zinc-500 uppercase">{text.wsl.config}</h3>
    <p class="text-sm text-zinc-500">{text.wsl.configHint}</p>
    <div class="mt-3 flex flex-wrap items-end gap-3">
      <label class="flex flex-col gap-1 text-sm">
        {text.wsl.memory}
        <input
          bind:value={config.memory}
          placeholder={text.wsl.memoryPlaceholder}
          class="w-28 rounded-md border border-zinc-300 bg-white px-2 py-1 dark:border-zinc-700 dark:bg-zinc-900"
        />
      </label>
      <label class="flex flex-col gap-1 text-sm">
        {text.wsl.processors}
        <input
          type="number"
          min="1"
          bind:value={config.processors}
          class="w-24 rounded-md border border-zinc-300 bg-white px-2 py-1 dark:border-zinc-700 dark:bg-zinc-900"
        />
      </label>
      <label class="flex flex-col gap-1 text-sm">
        {text.wsl.swap}
        <input
          bind:value={config.swap}
          placeholder={text.wsl.swapPlaceholder}
          class="w-28 rounded-md border border-zinc-300 bg-white px-2 py-1 dark:border-zinc-700 dark:bg-zinc-900"
        />
      </label>
      <button
        type="button"
        onclick={preview}
        class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
      >
        {text.wsl.preview}
      </button>
      {#if configPreview !== null}
        <button
          type="button"
          onclick={save}
          class="rounded-md bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-500"
        >
          {text.wsl.save}
        </button>
      {/if}
    </div>
    {#if configError}
      <p class="mt-2 text-sm text-red-600 dark:text-red-400">{configError}</p>
    {/if}
    {#if configMessage}
      <p class="mt-2 text-sm text-emerald-600 dark:text-emerald-400">{configMessage}</p>
    {/if}
    {#if configPreview !== null}
      <pre
        class="mt-2 max-h-48 overflow-auto rounded bg-zinc-100 p-2 font-mono text-xs dark:bg-zinc-900">{configPreview}</pre>
    {/if}
  </section>
{/if}
