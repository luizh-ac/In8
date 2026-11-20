<script lang="ts">
  import { exportBackup, importBackup } from "../api";
  import { message, profileSelection, store, type ApplyResult } from "../store.svelte";
  import { text, wslActionLabel } from "../strings";
  import type { ProfileEntry } from "../types";

  const t = text.profiles;
  const PREVIEW_APPS = 8;

  /** Feedback lines for the last action, shown at the top. */
  let notice = $state<{ lines: string[]; error: boolean } | null>(null);
  let busy = $state(false);
  let name = $state("");
  let confirmingDelete = $state<string | null>(null);

  const builtin = $derived(store.profiles.filter((p) => p.builtin));
  const mine = $derived(store.profiles.filter((p) => !p.builtin));
  const nameTaken = $derived(mine.some((p) => p.name.toLowerCase() === name.trim().toLowerCase()));

  function show(lines: string[], error = false): void {
    notice = { lines: lines.filter((line) => line !== ""), error };
  }

  function appNames(profile: ProfileEntry): string[] {
    return profile.apps.map((wanted) => {
      const app = store.catalog.find((a) => a.id === wanted.id);
      const label = app?.name ?? wanted.id;
      return store.isWindows && wanted.target === "wsl" ? `${label} (${text.targets.wsl})` : label;
    });
  }

  function describe(result: ApplyResult): string[] {
    return [
      t.applied.selected(result.selected),
      result.alreadyInstalled.length > 0 ? t.applied.already(result.alreadyInstalled) : "",
      result.skipped.length > 0 ? t.applied.skipped(result.skipped) : "",
      result.needsWslSetup ? t.applied.needsWsl : "",
    ];
  }

  function apply(profile: ProfileEntry): void {
    show(describe(store.applySelection(profileSelection(profile))));
  }

  function captureThisPc(): void {
    const added = store.selectInstalled();
    show([added === 0 ? t.capturedNone : t.captured(added)]);
  }

  async function run(work: () => Promise<void>): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await work();
    } catch (error) {
      show([message(error)], true);
    } finally {
      busy = false;
    }
  }

  function save(): Promise<void> {
    return run(async () => {
      const profileName = name.trim();
      await store.saveProfile(profileName);
      name = "";
      show([t.saved(profileName)]);
    });
  }

  function remove(id: string): Promise<void> {
    return run(async () => {
      await store.deleteProfile(id);
      confirmingDelete = null;
      notice = null;
    });
  }

  function doExport(): Promise<void> {
    return run(async () => {
      const path = await exportBackup(store.currentSelection());
      if (path) show([t.exported(path)]);
    });
  }

  function doImport(): Promise<void> {
    return run(async () => {
      const result = await importBackup();
      if (!result) return;
      await store.loadProfiles();
      const applied = store.applySelection(result.selection);
      show([
        t.imported(result.profiles),
        ...describe(applied),
        result.unknownApps.length > 0 ? t.unknownApps(result.unknownApps) : "",
      ]);
    });
  }
</script>

{#snippet profileCard(profile: ProfileEntry)}
  {@const names = appNames(profile)}
  <li class="flex flex-col rounded-lg border border-zinc-200 p-4 dark:border-zinc-800">
    <div class="flex items-start gap-3">
      <div class="min-w-0">
        <h4 class="font-medium">{profile.name}</h4>
        {#if profile.description}
          <p class="mt-0.5 text-sm text-zinc-500">{profile.description}</p>
        {/if}
        <p class="mt-1 text-xs text-zinc-400">{t.counts(
            profile.apps.length,
            store.isWindows ? profile.wsl.length : 0,
            profile.tweaks.filter((id) => store.tweaks.some((tweak) => tweak.id === id)).length,
            store.isWindows ? profile.debloat.length : 0,
            profile.postinstall.length,
          )}</p>
      </div>
      <button
        type="button"
        onclick={() => apply(profile)}
        class="ml-auto shrink-0 rounded-md bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-500"
      >
        {t.select}
      </button>
    </div>
    <ul class="mt-3 flex flex-wrap gap-1.5">
      {#each names.slice(0, PREVIEW_APPS) as appName (appName)}
        <li class="rounded bg-zinc-100 px-2 py-0.5 text-xs text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300">
          {appName}
        </li>
      {/each}
      {#if store.isWindows}
        {#each profile.wsl as action (action.kind)}
          <li class="rounded bg-indigo-50 px-2 py-0.5 text-xs text-indigo-700 dark:bg-indigo-950/50 dark:text-indigo-300">
            {wslActionLabel(action)}
          </li>
        {/each}
      {/if}
      {#if names.length > PREVIEW_APPS}
        <li class="px-1 py-0.5 text-xs text-zinc-400" title={names.slice(PREVIEW_APPS).join(", ")}>
          {t.more(names.length - PREVIEW_APPS)}
        </li>
      {/if}
    </ul>
    {#if !profile.builtin}
      <div class="mt-3 flex gap-3 text-xs">
        {#if confirmingDelete === profile.id}
          <button
            type="button"
            disabled={busy}
            onclick={() => remove(profile.id)}
            class="font-medium text-red-600 hover:text-red-500 dark:text-red-400"
          >
            {t.confirmDelete}
          </button>
          <button type="button" onclick={() => (confirmingDelete = null)} class="text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200">
            {t.cancel}
          </button>
        {:else}
          <button
            type="button"
            onclick={() => (confirmingDelete = profile.id)}
            class="text-zinc-500 hover:text-red-600 dark:hover:text-red-400"
          >
            {t.delete}
          </button>
        {/if}
      </div>
    {/if}
  </li>
{/snippet}

<div class="flex items-center gap-3">
  <h2 class="text-xl font-semibold">{t.title}</h2>
  <button
    type="button"
    disabled={busy}
    onclick={doImport}
    class="ml-auto rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100 disabled:opacity-50 dark:border-zinc-700 dark:hover:bg-zinc-800"
  >
    {t.import}
  </button>
  <button
    type="button"
    disabled={busy}
    onclick={doExport}
    class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100 disabled:opacity-50 dark:border-zinc-700 dark:hover:bg-zinc-800"
  >
    {t.export}
  </button>
</div>
<p class="mt-1 text-sm text-zinc-500">{t.intro}</p>
<p class="mt-1 text-xs text-zinc-400">{t.exportHint}</p>

{#if notice}
  <div
    class="mt-4 rounded-md p-3 text-sm {notice.error
      ? 'bg-red-50 text-red-700 dark:bg-red-950/40 dark:text-red-300'
      : 'bg-indigo-50 text-indigo-800 dark:bg-indigo-950/40 dark:text-indigo-200'}"
    role="status"
  >
    {#each notice.lines as line, i (i)}
      <p>{line}</p>
    {/each}
  </div>
{/if}

{#if store.profileError}
  <p class="mt-4 rounded-md bg-red-50 p-3 text-sm text-red-700 dark:bg-red-950/40 dark:text-red-300">
    {store.profileError}
  </p>
{/if}

<section class="mt-8">
  <h3 class="text-sm font-semibold tracking-wide text-zinc-500 uppercase">{t.thisPc}</h3>
  <div class="mt-3 flex items-center gap-4 rounded-lg border border-dashed border-zinc-300 p-4 dark:border-zinc-700">
    <p class="text-sm text-zinc-600 dark:text-zinc-400">
      {store.checkingInstalled || store.checkingWsl ? t.checking : t.thisPcHint}
    </p>
    <button
      type="button"
      disabled={store.checkingInstalled || store.checkingWsl}
      onclick={captureThisPc}
      class="ml-auto shrink-0 rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100 disabled:opacity-50 dark:border-zinc-700 dark:hover:bg-zinc-800"
    >
      {t.selectInstalled}
    </button>
  </div>
</section>

<section class="mt-8">
  <h3 class="text-sm font-semibold tracking-wide text-zinc-500 uppercase">{t.builtin}</h3>
  <ul class="mt-3 grid gap-3 lg:grid-cols-2">
    {#each builtin as profile (profile.id)}
      {@render profileCard(profile)}
    {/each}
  </ul>
</section>

<section class="mt-8">
  <h3 class="text-sm font-semibold tracking-wide text-zinc-500 uppercase">{t.mine}</h3>
  {#if mine.length === 0}
    <p class="mt-3 text-sm text-zinc-500">{t.noneSaved}</p>
  {:else}
    <ul class="mt-3 grid gap-3 lg:grid-cols-2">
      {#each mine as profile (profile.id)}
        {@render profileCard(profile)}
      {/each}
    </ul>
  {/if}

  <form
    class="mt-4 flex flex-wrap items-center gap-3"
    onsubmit={(event) => {
      event.preventDefault();
      save();
    }}
  >
    <label class="text-sm font-medium" for="profile-name">{t.save}</label>
    <input
      id="profile-name"
      bind:value={name}
      maxlength="60"
      placeholder={t.namePlaceholder}
      class="w-56 rounded-md border border-zinc-300 bg-white px-3 py-1.5 text-sm outline-none focus:border-indigo-500 dark:border-zinc-700 dark:bg-zinc-900"
    />
    <button
      type="submit"
      disabled={busy || name.trim() === "" || store.selectionCount === 0}
      class="rounded-md bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
    >
      {nameTaken ? t.replaceButton : t.saveButton}
    </button>
    <span class="text-xs text-zinc-400">
      {store.selectionCount === 0 ? t.nothingSelected : nameTaken ? t.saveHint : ""}
    </span>
  </form>
</section>
