<script lang="ts">
  import { postKey, store } from "../store.svelte";
  import { text } from "../strings.svelte";
  import type { InputKind, PostSideState, PostTask, Target } from "../types";

  const t = $derived(text.postinstall);
  let copied = $state<string | null>(null);

  const wslReady = $derived.by(() => {
    const active = store.wsl?.active;
    return store.isWindows && !!active && active.hasUser && active.apt;
  });

  function targetLabel(target: Target): string {
    return target === "wsl" ? t.wsl : t.hostLabel(store.isWindows);
  }

  function side(target: Target): PostSideState | null {
    return target === "wsl" ? (store.postState?.wsl ?? null) : (store.postState?.host ?? null);
  }

  /** Mirrors the backend's checks so problems show while typing (the backend checks again). */
  function problem(kind: InputKind, value: string): string | null {
    const v = value.trim();
    if (v === "") return t.invalid.required;
    switch (kind) {
      case "email":
        return /^[^\s@-][^\s@]*@[^\s@]+\.[^\s@.]+$/.test(v) ? null : t.invalid.email;
      case "gitUrl":
        return /^(https:\/\/|git@)\S+$/.test(v) ? null : t.invalid.gitUrl;
      case "folder":
        return !v.startsWith("-") && v.split("/").every((part) => /^[A-Za-z0-9._-]+$/.test(part) && part !== "." && part !== "..")
          ? null
          : t.invalid.folder;
      default:
        return null;
    }
  }

  /** The app the task needs, when it isn't installed or selected for any chosen target. */
  function missingRequirement(task: PostTask): string | null {
    if (!task.requires) return null;
    const app = store.catalog.find((a) => a.id === task.requires);
    if (!app) return null;
    const targets = store.postTargets(task.id).filter((target) => store.postSelected.has(postKey(task.id, target)));
    const missing = targets.some((target) => {
      // VS Code itself always lives on Windows, also for extensions that go to WSL.
      const appTarget: Target = task.requires === "vscode" ? "host" : target;
      const installed = appTarget === "wsl" ? store.installedWsl.has(app.id) : store.installed.has(app.id);
      const selected = store.selected.has(app.id) && store.targetOf(app) === appTarget;
      return !installed && !selected;
    });
    return missing ? app.name : null;
  }

  async function copy(key: string, value: string): Promise<void> {
    await navigator.clipboard.writeText(value);
    copied = key;
    setTimeout(() => {
      if (copied === key) copied = null;
    }, 1500);
  }
</script>

<div class="flex items-center gap-3">
  <h2 class="text-xl font-semibold">{t.title}</h2>
  <button
    type="button"
    disabled={store.checkingPost}
    onclick={() => store.refreshPostinstall()}
    class="ml-auto rounded-md border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100 disabled:opacity-50 dark:border-zinc-700 dark:hover:bg-zinc-800"
  >
    {store.checkingPost ? t.checking : t.checkAgain}
  </button>
</div>
<p class="mt-1 text-sm text-zinc-500">{t.intro}</p>
{#if store.isWindows && !wslReady}
  <p class="mt-1 text-xs text-zinc-400">{t.wslNotReady}</p>
{/if}

{#if store.postError}
  <p class="mt-4 rounded-md bg-red-50 p-3 text-sm text-red-700 dark:bg-red-950/40 dark:text-red-300">
    {store.postError}
  </p>
{/if}

<ul class="mt-6 flex flex-col gap-4">
  {#each store.postTasks as task (task.id)}
    {@const targets = store.postTargets(task.id)}
    {@const chosen = targets.filter((target) => store.postSelected.has(postKey(task.id, target)))}
    {@const values = store.postValues.get(task.id) ?? {}}
    {@const missing = missingRequirement(task)}
    {#if targets.length > 0}
      <li class="rounded-lg border p-4 {chosen.length > 0 ? 'border-indigo-400 dark:border-indigo-700' : 'border-zinc-200 dark:border-zinc-800'}">
        <div class="flex items-start gap-4">
          <div class="min-w-0 flex-1">
            <h3 class="text-sm font-medium">{task.name}</h3>
            <p class="mt-0.5 text-sm text-zinc-500">{task.description}</p>
          </div>
          <div class="flex shrink-0 gap-2">
            {#each targets as target (target)}
              {@const on = store.postSelected.has(postKey(task.id, target))}
              <button
                type="button"
                aria-pressed={on}
                onclick={() => store.togglePost(task.id, target)}
                class="rounded-md border px-3 py-1.5 text-sm {on
                  ? 'border-indigo-500 bg-indigo-50 text-indigo-800 dark:bg-indigo-950/40 dark:text-indigo-200'
                  : 'border-zinc-300 hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800'}"
              >
                {on ? "✓ " : ""}{targetLabel(target)}
              </button>
            {/each}
          </div>
        </div>

        {#if missing && chosen.length > 0}
          <p class="mt-2 text-xs text-amber-600 dark:text-amber-400">{t.needs(missing)}</p>
        {/if}

        {#if task.inputs.length > 0}
          <div class="mt-3 grid gap-3 sm:grid-cols-2">
            {#each task.inputs as input (input.id)}
              {@const value = values[input.id] ?? ""}
              {@const error = chosen.length > 0 ? problem(input.kind, value) : null}
              <label class="flex flex-col gap-1 text-sm">
                <span class="text-xs font-medium text-zinc-600 dark:text-zinc-400">{input.label}</span>
                <input
                  type={input.kind === "email" ? "email" : "text"}
                  value={value}
                  placeholder={input.placeholder || input.default}
                  oninput={(event) => store.setPostValue(task.id, input.id, event.currentTarget.value)}
                  class="rounded-md border bg-white px-3 py-1.5 outline-none focus:border-indigo-500 dark:bg-zinc-900 {error
                    ? 'border-red-400 dark:border-red-700'
                    : 'border-zinc-300 dark:border-zinc-700'}"
                />
                {#if error}
                  <span class="text-xs text-red-600 dark:text-red-400">{error}</span>
                {/if}
              </label>
            {/each}
          </div>
        {/if}

        {#if task.id === "git-identity"}
          {#each targets as target (target)}
            {@const state = side(target)}
            {#if state?.gitName || state?.gitEmail}
              <p class="mt-2 text-xs text-zinc-400">
                {t.current(targetLabel(target), [state.gitName, state.gitEmail].filter(Boolean).join(" · "))}
              </p>
            {/if}
          {/each}
        {/if}

        {#if task.id === "ssh-key"}
          {#each targets as target (target)}
            {@const key = side(target)?.sshPublicKey}
            {#if key}
              <div class="mt-3 rounded-md bg-zinc-100 p-2 dark:bg-zinc-900">
                <div class="flex items-center gap-2 text-xs">
                  <span class="font-medium">{t.publicKey(targetLabel(target))}</span>
                  <button
                    type="button"
                    onclick={() => copy(`ssh-${target}`, key)}
                    class="ml-auto rounded border border-zinc-300 px-2 py-0.5 hover:bg-zinc-200 dark:border-zinc-700 dark:hover:bg-zinc-800"
                  >
                    {copied === `ssh-${target}` ? t.copied : t.copy}
                  </button>
                </div>
                <code class="mt-1 block font-mono text-xs break-all text-zinc-600 dark:text-zinc-300">{key}</code>
                <p class="mt-1 text-xs text-zinc-400">{t.githubHint}</p>
              </div>
            {/if}
          {/each}
        {/if}

        {#if task.options.length > 0}
          {@const picked = store.postOptions.get(task.id) ?? []}
          <div class="mt-3 flex items-center gap-3 text-xs">
            <button type="button" class="text-indigo-600 hover:text-indigo-500 dark:text-indigo-400" onclick={() => store.postOptions.set(task.id, task.options.map((o) => o.id))}>
              {t.selectAll}
            </button>
            <button type="button" class="text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200" onclick={() => store.postOptions.set(task.id, [])}>
              {t.selectNone}
            </button>
          </div>
          <ul class="mt-2 grid gap-1 sm:grid-cols-2 lg:grid-cols-3">
            {#each task.options as option (option.id)}
              {@const installedOn = targets.filter((target) => side(target)?.extensions.includes(option.id.toLowerCase()))}
              <li>
                <label class="flex cursor-pointer items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={picked.includes(option.id)}
                    onchange={() => store.togglePostOption(task.id, option.id)}
                    class="accent-indigo-500"
                  />
                  <span>{option.label}</span>
                  {#if installedOn.length > 0}
                    <span class="text-xs text-emerald-600 dark:text-emerald-400">
                      {t.installed} ({installedOn.map(targetLabel).join(", ")})
                    </span>
                  {/if}
                </label>
              </li>
            {/each}
          </ul>
        {/if}
      </li>
    {/if}
  {/each}
</ul>
