<script lang="ts">
  import { store, type View } from "../store.svelte";
  import { text } from "../strings";

  const items = $derived(
    (
      [
        ["apps", text.nav.apps],
        ["profiles", text.nav.profiles],
        ["wsl", text.nav.wsl],
        ["debloat", text.nav.debloat],
        ["tweaks", text.nav.tweaks],
        ["postinstall", text.nav.postinstall],
        ["settings", text.nav.settings],
      ] as [View, string][]
    ).filter(([view]) => (view !== "wsl" && view !== "debloat") || store.platform?.os === "windows"),
  );

  const platformLine = $derived.by(() => {
    const p = store.platform;
    if (!p) return "";
    const system = p.distro?.name ?? (p.os === "windows" ? "Windows" : p.os);
    return `${system} · ${p.arch} · v${p.version}`;
  });
</script>

<aside class="flex w-56 shrink-0 flex-col border-r border-zinc-200 p-4 dark:border-zinc-800">
  <div class="mb-6 px-2">
    <h1 class="text-2xl font-semibold tracking-tight">In8</h1>
    <p class="text-xs text-zinc-500">{text.tagline}</p>
  </div>
  <nav class="flex flex-col gap-1">
    {#each items as [view, label] (view)}
      {@const active = store.view === view || (view === "apps" && store.view === "run")}
      <button
        type="button"
        class="rounded-md px-3 py-2 text-left text-sm transition-colors
          {active ? 'bg-zinc-200 font-medium dark:bg-zinc-800' : 'hover:bg-zinc-100 dark:hover:bg-zinc-900'}"
        aria-current={active ? "page" : undefined}
        onclick={() => (store.view = view)}
      >
        {label}
      </button>
    {/each}
  </nav>
  <p class="mt-auto px-2 text-xs text-zinc-500">{platformLine}</p>
</aside>
