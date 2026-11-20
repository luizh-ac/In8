<script lang="ts">
  import { message, store } from "../store.svelte";
  import { languages, text } from "../strings.svelte";
  import type { Settings, Theme } from "../types";

  const t = $derived(text.settings);
  const themes: Theme[] = ["system", "light", "dark"];

  let error = $state<string | null>(null);
  let restarting = $state(false);
  let declined = $state(false);

  async function update(patch: Partial<Settings>): Promise<void> {
    error = null;
    try {
      await store.updateSettings(patch);
    } catch (e) {
      error = message(e);
    }
  }

  async function restart(): Promise<void> {
    restarting = true;
    declined = false;
    try {
      declined = !(await store.restartAsAdmin());
    } catch (e) {
      error = message(e);
    } finally {
      restarting = false;
    }
  }
</script>

<h2 class="text-xl font-semibold">{t.title}</h2>

{#if error}
  <p class="mt-4 rounded-md bg-red-50 p-3 text-sm text-red-700 dark:bg-red-950/40 dark:text-red-300">{error}</p>
{/if}

<div class="mt-6 flex max-w-2xl flex-col divide-y divide-zinc-200 rounded-lg border border-zinc-200 dark:divide-zinc-800 dark:border-zinc-800">
  {#if store.admin.canElevate}
    <div class="px-4 py-4">
      <label class="flex items-start gap-3">
        <input
          type="checkbox"
          checked={store.settings.alwaysAdmin}
          onchange={(event) => update({ alwaysAdmin: event.currentTarget.checked })}
          class="mt-1 accent-indigo-500"
        />
        <span>
          <span class="block text-sm font-medium">{t.alwaysAdmin}</span>
          <span class="block text-xs text-zinc-500">{t.alwaysAdminHint}</span>
          <span class="block text-xs text-zinc-400">{t.alwaysAdminDev}</span>
        </span>
      </label>
      <div class="mt-3 ml-7 flex flex-wrap items-center gap-3 text-xs">
        <span class={store.admin.elevated ? "text-emerald-600 dark:text-emerald-400" : "text-zinc-500"}>
          {store.admin.elevated ? t.elevated : t.notElevated}
        </span>
        {#if !store.admin.elevated}
          <button
            type="button"
            disabled={restarting || store.running}
            onclick={restart}
            class="rounded-md border border-zinc-300 px-2.5 py-1 hover:bg-zinc-100 disabled:opacity-50 dark:border-zinc-700 dark:hover:bg-zinc-800"
          >
            {t.restartNow}
          </button>
        {/if}
      </div>
      {#if store.admin.declined || declined}
        <p class="mt-2 ml-7 text-xs text-amber-600 dark:text-amber-400">{t.declined}</p>
      {/if}
    </div>
  {/if}

  <label class="flex items-start gap-3 px-4 py-4">
    <input
      type="checkbox"
      checked={store.settings.dryRunByDefault}
      onchange={(event) => update({ dryRunByDefault: event.currentTarget.checked })}
      class="mt-1 accent-indigo-500"
    />
    <span>
      <span class="block text-sm font-medium">{t.dryRun}</span>
      <span class="block text-xs text-zinc-500">{t.dryRunHint}</span>
    </span>
  </label>

  <label class="flex items-center gap-4 px-4 py-4">
    <span>
      <span class="block text-sm font-medium">{t.language}</span>
      <span class="block text-xs text-zinc-500">{t.languageHint}</span>
    </span>
    <select
      value={store.settings.language}
      onchange={(event) => update({ language: event.currentTarget.value as Settings["language"] })}
      class="ml-auto rounded-md border border-zinc-300 bg-white px-2 py-1.5 text-sm dark:border-zinc-700 dark:bg-zinc-900"
    >
      <option value="system">{t.languageSystem}</option>
      {#each languages as language (language.value)}
        <option value={language.value}>{language.label}</option>
      {/each}
    </select>
  </label>

  <div class="flex items-center gap-4 px-4 py-4">
    <span class="text-sm font-medium">{t.theme}</span>
    <div class="ml-auto flex rounded-md border border-zinc-300 p-0.5 dark:border-zinc-700" role="radiogroup" aria-label={t.theme}>
      {#each themes as theme (theme)}
        <button
          type="button"
          role="radio"
          aria-checked={store.settings.theme === theme}
          onclick={() => update({ theme })}
          class="rounded px-3 py-1 text-sm {store.settings.theme === theme
            ? 'bg-zinc-200 font-medium dark:bg-zinc-700'
            : 'text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200'}"
        >
          {t.themes[theme]}
        </button>
      {/each}
    </div>
  </div>

  {#if store.platform?.os === "linux"}
    <label class="flex items-start gap-3 px-4 py-4">
      <input
        type="checkbox"
        checked={store.settings.linuxPreferNative}
        onchange={(event) => update({ linuxPreferNative: event.currentTarget.checked })}
        class="mt-1 accent-indigo-500"
      />
      <span>
        <span class="block text-sm font-medium">{t.preferNative}</span>
        <span class="block text-xs text-zinc-500">{t.preferNativeHint}</span>
      </span>
    </label>
  {/if}

  {#if store.isWindows}
    <div class="flex items-center gap-4 px-4 py-4">
      <span>
        <span class="block text-sm font-medium">{t.wslDistro}</span>
        <span class="block text-xs text-zinc-500">{t.wslDistroHint}</span>
      </span>
      <span class="ml-auto text-sm text-zinc-600 dark:text-zinc-300">
        {store.settings.wslDistro ?? t.wslDistroDefault}
      </span>
    </div>
  {/if}
</div>
