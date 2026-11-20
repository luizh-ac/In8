<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import { message, store, type TaskState } from "../store.svelte";
  import { sourceLabel, taskStatusLabels, text, wslActionLabel } from "../strings.svelte";

  const expanded = new SvelteSet<string>();
  const started = $derived(store.running || store.tasks.length > 0 || store.runError !== null);
  const advancedDebloat = $derived(store.selectedDebloat.filter((item) => item.level === "advanced"));
  let confirmedAdvanced = $state(false);
  /** Native Linux installs go through pkexec: one password prompt for the run. */
  const asksPassword = $derived(
    store.platform?.os === "linux" &&
      (store.setupFlatpak ||
        store.selectedApps.some((app) => {
          const resolution = store.resolutionFor(app);
          return resolution?.status === "available" && ["apt", "dnf", "pacman"].includes(resolution.manager);
        })),
  );
  const postItems = $derived(
    [...store.postSelected].map((key) => {
      const [id, target] = key.split("@");
      const name = store.postTasks.find((task) => task.id === id)?.name ?? id;
      const where = target === "wsl" ? text.targets.wsl : text.postinstall.hostLabel(store.isWindows);
      return { key, label: text.postinstall.reviewLabel(name, where) };
    }),
  );
  let restarting = $state(false);
  let adminDeclined = $state(false);

  /** A finished tweak task whose tweak only shows up fully after signing out. */
  const signOutNeeded = $derived(
    store.tasks.some(
      (task) =>
        task.status === "done" &&
        task.id.startsWith("tweak-") &&
        store.tweaks.find((tweak) => `tweak-${tweak.id}` === task.id)?.signOut,
    ),
  );

  async function restartAsAdmin(): Promise<void> {
    restarting = true;
    adminDeclined = false;
    try {
      adminDeclined = !(await store.restartAsAdmin());
    } catch (error) {
      store.runError = message(error);
    } finally {
      restarting = false;
    }
  }

  function toggleLog(id: string): void {
    if (expanded.has(id)) expanded.delete(id);
    else expanded.add(id);
  }

  function statusClass(status: TaskState["status"]): string {
    switch (status) {
      case "succeeded":
      case "done":
      case "removed":
      case "alreadyInstalled":
        return "text-emerald-600 dark:text-emerald-400";
      case "failed":
        return "text-red-600 dark:text-red-400";
      case "simulated":
        return "text-indigo-600 dark:text-indigo-400";
      default:
        return "text-zinc-500";
    }
  }

  function backToApps(): void {
    if (store.running) return;
    store.resetRun();
    store.view = store.previousView;
  }
</script>

<div class="flex items-center gap-3">
  <button
    type="button"
    onclick={backToApps}
    disabled={store.running}
    class="rounded-md px-2 py-1 text-sm text-zinc-500 hover:bg-zinc-100 disabled:opacity-50 dark:hover:bg-zinc-800"
  >
    ← {text.run.back}
  </button>
  <h2 class="text-xl font-semibold">{text.run.title}</h2>
</div>

{#if !started}
  {#if store.selectionCount === 0}
    <p class="mt-6 text-sm text-zinc-500">{text.run.empty}</p>
  {:else}
    {#if store.needsAdmin}
      <div class="mt-6 flex flex-wrap items-center gap-3 rounded-md bg-amber-50 p-3 text-sm text-amber-800 dark:bg-amber-950/40 dark:text-amber-200">
        <p class="flex-1">{text.run.needsAdmin}</p>
        <button
          type="button"
          disabled={restarting}
          onclick={restartAsAdmin}
          class="rounded-md bg-amber-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-500 disabled:opacity-50"
        >
          {text.run.restartAsAdmin}
        </button>
        {#if adminDeclined}
          <p class="w-full text-xs">{text.run.adminDeclined}</p>
        {/if}
      </div>
    {/if}

    {#if asksPassword}
      <p class="mt-6 rounded-md bg-zinc-100 p-3 text-sm text-zinc-600 dark:bg-zinc-900 dark:text-zinc-300">
        {text.run.askPassword}
      </p>
    {/if}

    <ul class="mt-6 divide-y divide-zinc-200 rounded-lg border border-zinc-200 dark:divide-zinc-800 dark:border-zinc-800">
      {#if store.repairWinget}
        <li class="flex items-center gap-3 px-4 py-2 text-sm">
          <span class="font-medium">{text.run.repairWinget}</span>
        </li>
      {/if}
      {#if store.setupFlatpak}
        <li class="flex items-center gap-3 px-4 py-2 text-sm">
          <span class="font-medium">{text.run.setupFlatpak}</span>
        </li>
      {/if}
      {#each store.selectedDebloat as item (item.id)}
        <li class="flex items-center gap-3 px-4 py-2 text-sm">
          <span class="font-medium">{text.run.groups.debloat}: {item.name}</span>
          {#if item.level === "advanced"}
            <span class="text-xs text-amber-600 dark:text-amber-400">{text.debloat.advanced}</span>
          {/if}
          <span class="ml-auto text-xs text-zinc-400">{item.winget ? "winget" : "Appx"}</span>
        </li>
      {/each}
      {#each store.selectedApps as app (app.id)}
        {@const target = store.targetOf(app)}
        {@const resolution = store.resolutionFor(app)}
        <li class="flex items-center gap-3 px-4 py-2 text-sm">
          <span class="font-medium">{app.name}</span>
          {#if resolution?.status === "available"}
            <span class="text-zinc-500">{resolution.package}</span>
          {/if}
          <span class="ml-auto text-xs text-zinc-400">{sourceLabel(app, target)}</span>
        </li>
      {/each}
      {#each store.pendingWslActions as action (action.kind)}
        <li class="flex items-center gap-3 px-4 py-2 text-sm">
          <span class="font-medium">{wslActionLabel(action)}</span>
          <span class="ml-auto text-xs text-zinc-400">{text.targets.wsl}</span>
        </li>
      {/each}
      {#each postItems as item (item.key)}
        <li class="flex items-center gap-3 px-4 py-2 text-sm">
          <span class="font-medium">{item.label}</span>
          <span class="ml-auto text-xs text-zinc-400">{text.run.groups.postinstall}</span>
        </li>
      {/each}
      {#each store.selectedTweaks as tweak (tweak.id)}
        <li class="flex items-center gap-3 px-4 py-2 text-sm">
          <span class="font-medium">
            {store.tweakChoices.get(tweak.id) === "revert" ? text.run.revertLabel(tweak.name) : tweak.name}
          </span>
          {#if tweak.requiresAdmin}
            <span class="text-xs text-amber-600 dark:text-amber-400">{text.tweaks.admin}</span>
          {/if}
          <span class="ml-auto text-xs text-zinc-400">{text.run.groups.tweaks}</span>
        </li>
      {/each}
    </ul>

    {#if advancedDebloat.length > 0}
      <label class="mt-6 flex items-start gap-3 rounded-md border border-amber-300 p-3 dark:border-amber-800">
        <input type="checkbox" bind:checked={confirmedAdvanced} class="mt-1 accent-amber-500" />
        <span class="text-sm">{text.run.confirmAdvanced(advancedDebloat.map((item) => item.name))}</span>
      </label>
    {/if}

    <label class="mt-6 flex items-start gap-3">
      <input type="checkbox" bind:checked={store.dryRun} class="mt-1 accent-indigo-500" />
      <span>
        <span class="block text-sm font-medium">{text.run.dryRun}</span>
        <span class="block text-xs text-zinc-500">{text.run.dryRunHint}</span>
      </span>
    </label>

    <button
      type="button"
      disabled={advancedDebloat.length > 0 && !confirmedAdvanced}
      onclick={() => store.run()}
      class="mt-6 rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
    >
      {store.dryRun ? text.run.simulate : text.run.start}
    </button>
  {/if}
{:else}
  {#if store.runError}
    <p class="mt-6 rounded-md bg-red-50 p-3 text-sm text-red-700 dark:bg-red-950/40 dark:text-red-300">
      {store.runError}
    </p>
  {/if}

  <ul class="mt-6 flex flex-col gap-2">
    {#each store.tasks as task (task.id)}
      <li class="rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
        <div class="flex items-center gap-3">
          {#if task.status === "running"}
            <span
              class="size-3 shrink-0 animate-spin rounded-full border-2 border-indigo-500 border-t-transparent"
              aria-hidden="true"
            ></span>
          {/if}
          <span class="text-sm font-medium">{task.label}</span>
          <span class="text-xs {statusClass(task.status)}">
            {taskStatusLabels[task.status]}{task.exitCode !== null && task.status === "failed"
              ? ` (exit ${task.exitCode})`
              : ""}
          </span>
          <button
            type="button"
            onclick={() => toggleLog(task.id)}
            class="ml-auto text-xs text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200"
          >
            {expanded.has(task.id) || task.status === "running" ? text.run.hideLog : text.run.showLog}
          </button>
        </div>
        <code class="mt-2 block font-mono text-xs break-all text-zinc-500">{task.command}</code>
        {#if expanded.has(task.id) || task.status === "running"}
          <pre
            class="mt-2 max-h-48 overflow-auto rounded bg-zinc-100 p-2 font-mono text-xs whitespace-pre-wrap dark:bg-zinc-900">{task
              .lines.length > 0
              ? task.lines.join("\n")
              : text.run.noOutput}</pre>
        {/if}
      </li>
    {/each}
  </ul>

  {#if store.running}
    <p class="mt-4 text-sm text-zinc-500">{text.run.running}</p>
  {:else if store.summary}
    <div class="mt-6 flex items-center gap-4">
      <p class="text-sm font-medium">{text.run.summary(store.summary.succeeded, store.summary.failed)}</p>
      {#if signOutNeeded}
        <p class="text-sm text-zinc-500">{text.run.signOut}</p>
      {/if}
      <button
        type="button"
        onclick={() => store.resetRun()}
        class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
      >
        {text.run.runAgain}
      </button>
    </div>
  {/if}
{/if}
