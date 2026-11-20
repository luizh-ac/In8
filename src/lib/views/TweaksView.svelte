<script lang="ts">
  import { store } from "../store.svelte";
  import { text, tweakCategoryLabels, tweakCategoryOrder } from "../strings.svelte";
  import type { TweakInfo } from "../types";

  const t = $derived(text.tweaks);

  const groups = $derived(
    tweakCategoryOrder
      .map((category) => ({ category, tweaks: store.tweaks.filter((tweak) => tweak.category === category) }))
      .filter((group) => group.tweaks.length > 0),
  );

  /** Selects every tweak in the group that isn't on yet. */
  function selectGroup(tweaks: TweakInfo[]): void {
    for (const tweak of tweaks) {
      if (!tweak.applied) store.tweakChoices.set(tweak.id, "apply");
    }
  }
</script>

<div class="flex items-center gap-3">
  <h2 class="text-xl font-semibold">{t.title}</h2>
  <button
    type="button"
    disabled={store.checkingTweaks}
    onclick={() => store.refreshTweaks()}
    class="ml-auto rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100 disabled:opacity-50 dark:border-zinc-700 dark:hover:bg-zinc-800"
  >
    {store.checkingTweaks ? t.checking : t.checkAgain}
  </button>
</div>
<p class="mt-1 text-sm text-zinc-500">{t.intro}</p>

{#if store.tweakError}
  <p class="mt-4 rounded-md bg-red-50 p-3 text-sm text-red-700 dark:bg-red-950/40 dark:text-red-300">
    {store.tweakError}
  </p>
{:else if !store.checkingTweaks && store.tweaks.length === 0}
  <p class="mt-6 text-sm text-zinc-500">{t.none}</p>
{/if}

{#each groups as group (group.category)}
  <section class="mt-8">
    <div class="flex items-center gap-3">
      <h3 class="text-sm font-semibold tracking-wide text-zinc-500 uppercase">
        {tweakCategoryLabels[group.category]}
      </h3>
      {#if group.tweaks.some((tweak) => !tweak.applied)}
        <button
          type="button"
          onclick={() => selectGroup(group.tweaks)}
          class="text-xs text-indigo-600 hover:text-indigo-500 dark:text-indigo-400"
        >
          {t.selectCategory}
        </button>
      {/if}
    </div>
    <ul class="mt-3 divide-y divide-zinc-200 rounded-lg border border-zinc-200 dark:divide-zinc-800 dark:border-zinc-800">
      {#each group.tweaks as tweak (tweak.id)}
        {@const choice = store.tweakChoices.get(tweak.id)}
        <li class="flex items-center gap-4 px-4 py-3">
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-sm font-medium">{tweak.name}</span>
              {#if tweak.applied}
                <span class="rounded bg-emerald-50 px-1.5 py-0.5 text-xs text-emerald-700 dark:bg-emerald-950/50 dark:text-emerald-300">
                  {t.applied}
                </span>
              {/if}
              {#if tweak.requiresAdmin}
                <span class="rounded bg-amber-50 px-1.5 py-0.5 text-xs text-amber-700 dark:bg-amber-950/50 dark:text-amber-300">
                  {t.admin}
                </span>
              {/if}
              {#if tweak.signOut}
                <span class="rounded bg-zinc-100 px-1.5 py-0.5 text-xs text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300" title={t.signOutHint}>
                  {t.signOut}
                </span>
              {/if}
            </div>
            <p class="mt-0.5 text-sm text-zinc-500">{tweak.description}</p>
          </div>
          <div class="flex shrink-0 gap-2">
            {#if tweak.revertible}
              <button
                type="button"
                aria-pressed={choice === "revert"}
                onclick={() => store.chooseTweak(tweak.id, "revert")}
                class="rounded-md border px-3 py-1.5 text-sm {choice === 'revert'
                  ? 'border-amber-500 bg-amber-50 text-amber-800 dark:bg-amber-950/40 dark:text-amber-200'
                  : 'border-zinc-300 hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800'}"
              >
                {choice === "revert" ? t.willRevert : t.revert}
              </button>
            {/if}
            {#if !tweak.applied || choice === "apply"}
              <button
                type="button"
                aria-pressed={choice === "apply"}
                onclick={() => store.chooseTweak(tweak.id, "apply")}
                class="rounded-md border px-3 py-1.5 text-sm {choice === 'apply'
                  ? 'border-indigo-500 bg-indigo-50 text-indigo-800 dark:bg-indigo-950/40 dark:text-indigo-200'
                  : 'border-zinc-300 hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800'}"
              >
                {choice === "apply" ? t.willApply : t.apply}
              </button>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  </section>
{/each}
