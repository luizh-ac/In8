// All user-facing text lives here (English for now, ready for i18n later).
import type { CatalogApp, Category, TaskStatus } from "./types";

export const categoryOrder: Category[] = [
  "browsers",
  "dev",
  "communication",
  "media",
  "utilities",
  "gaming",
  "productivity",
  "security",
];

export const categoryLabels: Record<Category, string> = {
  browsers: "Browsers",
  dev: "Development",
  communication: "Communication",
  media: "Media",
  utilities: "Utilities",
  gaming: "Gaming",
  productivity: "Productivity",
  security: "Security",
};

export const taskStatusLabels: Record<TaskStatus | "pending" | "running", string> = {
  pending: "Waiting",
  running: "Running",
  succeeded: "Installed",
  alreadyInstalled: "Already installed",
  failed: "Failed",
  simulated: "Simulated",
};

/** Where the app comes from, e.g. "winget", "Microsoft Store", "GitHub", "Flathub". */
export function sourceLabel(app: CatalogApp): string | null {
  if (app.resolution.status !== "available") return null;
  switch (app.resolution.manager) {
    case "github":
      return "GitHub release";
    case "flatpak": {
      const flatpak = app.packages.flatpak;
      return typeof flatpak === "object" ? `Flatpak (${flatpak.remote.name})` : "Flathub";
    }
    case "winget": {
      const winget = app.packages.winget;
      return typeof winget === "object" && winget.source === "msstore" ? "Microsoft Store" : "winget";
    }
  }
}

export const text = {
  tagline: "Your PC, ready from birth.",
  comingSoon: "Coming in a later phase.",
  nav: {
    apps: "Apps",
    profiles: "Profiles",
    wsl: "WSL",
    debloat: "Debloat",
    tweaks: "Tweaks",
    postinstall: "Post-install",
    settings: "Settings",
  },
  apps: {
    title: "Apps",
    searchPlaceholder: "Search apps",
    hideUnavailable: "Hide unavailable",
    checkInstalled: "Check installed",
    checkingInstalled: "Checking installed apps…",
    installed: "Installed",
    noResults: "No apps match your search.",
    noManagers: "No supported package manager was found on this system.",
  },
  bar: {
    selected: (count: number) => (count === 1 ? "1 app selected" : `${count} apps selected`),
    clear: "Clear",
    review: "Review and run",
  },
  run: {
    title: "Review and run",
    back: "Back to apps",
    empty: "Select at least one app to install.",
    dryRun: "Dry run",
    dryRunHint: "Show the exact commands without installing anything.",
    start: "Install",
    simulate: "Simulate",
    running: "Running…",
    runAgain: "Run again",
    summary: (succeeded: number, failed: number) =>
      failed === 0 ? `All ${succeeded} tasks finished.` : `${succeeded} finished, ${failed} failed.`,
    showLog: "Show output",
    hideLog: "Hide output",
    noOutput: "No output.",
  },
} as const;
