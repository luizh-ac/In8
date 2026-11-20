// All user-facing text lives here (English for now, ready for i18n later).
import type { CatalogApp, Category, Target, TaskStatus, WslAction } from "./types";

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
  done: "Done",
  alreadyInstalled: "Already installed",
  failed: "Failed",
  simulated: "Simulated",
};

/** Where the app comes from, e.g. "winget", "Microsoft Store", "GitHub", "Flathub", "apt in WSL". */
export function sourceLabel(app: CatalogApp, target: Target = "host"): string | null {
  if (target === "wsl") return app.wslResolution?.status === "available" ? "apt in WSL" : null;
  if (app.resolution.status !== "available") return null;
  switch (app.resolution.manager) {
    case "apt":
      return "apt";
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

export function wslActionLabel(action: WslAction): string {
  switch (action.kind) {
    case "installWsl":
      return "Install WSL";
    case "installDistro":
      return `Install the ${action.distro} distro`;
    case "enableSystemd":
      return "Turn on systemd";
    case "dockerEngine":
      return "Docker Engine in WSL";
    case "vscodeExtension":
      return "VS Code WSL extension";
  }
}

export const text = {
  tagline: "Your PC, ready from birth.",
  targets: { host: "Windows", wsl: "WSL" } satisfies Record<Target, string>,
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
  wsl: {
    title: "WSL",
    intro:
      "Keep languages and dev tools inside Linux and Windows clean. VS Code stays on Windows and connects to WSL.",
    checking: "Checking WSL…",
    checkAgain: "Check again",
    notInstalled: "WSL isn't installed.",
    installWsl: "Install WSL",
    installWslHint: "Needs administrator rights. Windows may ask you to restart; open In8 again afterwards.",
    version: (v: string) => `WSL ${v}`,
    noDistro: "No Linux distro yet.",
    distro: "Distro",
    useDistro: "Use this distro",
    installDistro: "Install a distro",
    loadDistros: "Show available distros",
    loadingDistros: "Loading…",
    pickDistro: "Choose a distro",
    distroQueued: (name: string) => `${name} will be installed when you run.`,
    createUser: (name: string) =>
      `${name} doesn't have a Linux user yet. Open it, choose a user name and password, then check again.`,
    openDistro: (name: string) => `Open ${name}`,
    notApt: "This distro doesn't use apt. In8 can only install into Ubuntu or Debian distros for now.",
    details: {
      user: "User",
      systemd: "systemd",
      docker: "Docker Engine",
      on: "On",
      off: "Off",
      installed: "Installed",
      notInstalled: "Not installed",
    },
    environment: "Environment",
    systemd: "Turn on systemd",
    systemdHint: "Needed for Docker and other services. Restarts the distro.",
    docker: "Docker Engine in WSL",
    dockerHint: "docker.io and docker compose from Ubuntu, started by systemd. No Docker Desktop needed.",
    dockerNeedsSystemd: "Needs systemd. Turn it on too.",
    dockerDesktop:
      "Docker Desktop is installed. Running both can conflict; consider uninstalling Docker Desktop after Docker Engine works.",
    vscode: "VS Code WSL extension",
    vscodeHint: "Lets you run `code .` inside WSL to open the folder in VS Code.",
    vscodeMissing: "VS Code isn't installed. Select it in Apps to install it in the same run.",
    done: "Done",
    devTools: "Dev tools",
    devToolsHint: "Choose Windows or WSL on each dev app in Apps, or move the common ones in one click.",
    devToolsButton: "Use WSL for Git, Node.js, Python and Rust",
    projectsTip: "Keep your projects in ~/ inside Linux, not in /mnt/c. Windows files are much slower from WSL.",
    config: ".wslconfig",
    configHint: "Resources for the WSL virtual machine. Applies the next time WSL starts.",
    memory: "Memory",
    memoryPlaceholder: "8GB",
    processors: "Processors",
    swap: "Swap",
    swapPlaceholder: "4GB or 0",
    preview: "Preview",
    save: "Save",
    saved: "Saved. Restart WSL (wsl --shutdown) to apply.",
  },
  profiles: {
    title: "Profiles",
    intro: "Pick one or more. Each adds its apps to your selection; apps already installed stay unselected.",
    import: "Import…",
    export: "Export…",
    exportHint:
      "Export saves your current selection and your saved profiles to one file. Keep it somewhere that survives formatting, like a cloud drive or a USB stick.",
    exported: (path: string) => `Saved to ${path}`,
    imported: (profiles: number) =>
      profiles === 0
        ? "Backup imported."
        : `Backup imported with ${profiles === 1 ? "1 saved profile" : `${profiles} saved profiles`}.`,
    unknownApps: (ids: string[]) => `Not in this version of In8: ${ids.join(", ")}.`,
    thisPc: "This PC",
    thisPcHint: "Select every catalog app installed here, including inside WSL. Then save it as a profile or export it.",
    selectInstalled: "Select installed apps",
    checking: "Still checking what's installed…",
    capturedNone: "Nothing new to select.",
    captured: (count: number) => (count === 1 ? "Selected 1 item." : `Selected ${count} items.`),
    builtin: "Ready-made",
    mine: "My profiles",
    noneSaved: "No saved profiles yet. Select apps, then save them below.",
    select: "Select",
    delete: "Delete",
    confirmDelete: "Confirm delete",
    cancel: "Cancel",
    counts: (apps: number, wsl: number) =>
      [apps === 1 ? "1 app" : `${apps} apps`, wsl > 0 ? (wsl === 1 ? "1 WSL option" : `${wsl} WSL options`) : null]
        .filter(Boolean)
        .join(" · "),
    more: (count: number) => `+${count} more`,
    save: "Save current selection",
    saveHint: "Saving with the name of an existing profile replaces it.",
    namePlaceholder: "Profile name",
    saveButton: "Save",
    replaceButton: "Replace",
    saved: (name: string) => `Saved "${name}".`,
    nothingSelected: "Select apps or WSL options first.",
    applied: {
      selected: (count: number) =>
        count === 0 ? "Nothing new to select." : count === 1 ? "Selected 1 item." : `Selected ${count} items.`,
      already: (names: string[]) => `Already installed: ${names.join(", ")}.`,
      skipped: (names: string[]) => `Not available on this system: ${names.join(", ")}.`,
      needsWsl: "The WSL parts need WSL and a distro with a Linux user. Set them up in the WSL screen, then select this profile again.",
    },
  },
  bar: {
    selected: (count: number) => (count === 1 ? "1 item selected" : `${count} items selected`),
    clear: "Clear",
    review: "Review and run",
  },
  run: {
    title: "Review and run",
    back: "Back",
    empty: "Select at least one app or WSL option.",
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
