<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import { store, type TaskState } from "../store.svelte";
  import { sourceLabel, taskStatusLabels, text, wslActionLabel } from "../strings";

  const expanded = new SvelteSet<string>();
  const started = $derived(store.running || store.tasks.length > 0 || store.runError !== null);

  function toggleLog(id: string): void {
    if (expanded.has(id)) expanded.delete(id);
    else expanded.add(id);
  }

  function statusClass(status: TaskState["status"]): string {
    switch (status) {
      case "succeeded":
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
    <ul class="mt-6 divide-y divide-zinc-200 rounded-lg border border-zinc-200 dark:divide-zinc-800 dark:border-zinc-800">
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
    </ul>

    <label class="mt-6 flex items-start gap-3">
      <input type="checkbox" bind:checked={store.dryRun} class="mt-1 accent-indigo-500" />
      <span>
        <span class="block text-sm font-medium">{text.run.dryRun}</span>
        <span class="block text-xs text-zinc-500">{text.run.dryRunHint}</span>
      </span>
    </label>

    <button
      type="button"
      onclick={() => store.run()}
      class="mt-6 rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500"
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
