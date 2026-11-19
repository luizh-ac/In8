<script lang="ts">
  import { getPlatform } from "./lib/api";
  import type { PlatformInfo } from "./lib/types";

  const sections = ["Apps", "Profiles", "Debloat", "Tweaks", "Post-install", "Settings"];

  let active = $state("Apps");
  let platform = $state<PlatformInfo | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    getPlatform()
      .then((info) => (platform = info))
      .catch((e: unknown) => (error = String(e)));
  });
</script>

<div class="flex h-full bg-zinc-50 text-zinc-900 dark:bg-zinc-950 dark:text-zinc-100">
  <aside class="flex w-56 flex-col border-r border-zinc-200 p-4 dark:border-zinc-800">
    <div class="mb-6 px-2">
      <h1 class="text-2xl font-semibold tracking-tight">In8</h1>
      <p class="text-xs text-zinc-500">Your PC, ready from birth.</p>
    </div>
    <nav class="flex flex-col gap-1">
      {#each sections as section (section)}
        <button
          type="button"
          class="rounded-md px-3 py-2 text-left text-sm transition-colors
            {active === section
            ? 'bg-zinc-200 font-medium dark:bg-zinc-800'
            : 'hover:bg-zinc-100 dark:hover:bg-zinc-900'}"
          onclick={() => (active = section)}
        >
          {section}
        </button>
      {/each}
    </nav>
    <div class="mt-auto px-2 text-xs text-zinc-500">
      {#if platform}
        {platform.os} · {platform.arch} · v{platform.version}
      {:else if error}
        <span class="text-red-500">Backend error: {error}</span>
      {:else}
        Detecting platform…
      {/if}
    </div>
  </aside>

  <main class="flex-1 p-8">
    <h2 class="text-xl font-semibold">{active}</h2>
    <p class="mt-2 text-sm text-zinc-500">Coming soon.</p>
  </main>
</div>
