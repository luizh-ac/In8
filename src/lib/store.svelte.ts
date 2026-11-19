// App-wide state (Svelte 5 runes).
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { getCatalog, getInstalled, getPlatform, getWslInstalled, getWslStatus, startRun } from "./api";
import type {
  CatalogApp,
  PlatformInfo,
  Resolution,
  RunEvent,
  RunRequest,
  RunSummary,
  Target,
  TaskStatus,
  WslAction,
  WslStatus,
} from "./types";

export type View =
  | "apps"
  | "profiles"
  | "wsl"
  | "debloat"
  | "tweaks"
  | "postinstall"
  | "settings"
  | "run";

/** WSL actions that are simple on/off choices (installing a distro also needs a name). */
export type WslToggle = "installWsl" | "enableSystemd" | "dockerEngine" | "vscodeExtension";

export interface TaskState {
  id: string;
  label: string;
  command: string;
  status: TaskStatus | "running";
  exitCode: number | null;
  lines: string[];
}

const MAX_LINES_PER_TASK = 500;
/** Apps the "Dev tools on WSL" shortcut moves into WSL. */
export const DEV_ON_WSL = ["git", "nodejs", "python", "rust"];

export function message(error: unknown): string {
  return typeof error === "string" ? error : error instanceof Error ? error.message : String(error);
}

class AppStore {
  view = $state<View>("apps");
  /** Where "Back" from the run screen returns to. */
  previousView = $state<View>("apps");
  platform = $state<PlatformInfo | null>(null);
  catalog = $state<CatalogApp[]>([]);
  loadError = $state<string | null>(null);

  installed = new SvelteSet<string>();
  installedWsl = new SvelteSet<string>();
  checkingInstalled = $state(false);

  selected = new SvelteSet<string>();
  /** Chosen install target per app; apps not listed install on the host. */
  targets = new SvelteMap<string, Target>();

  wsl = $state<WslStatus | null>(null);
  checkingWsl = $state(false);
  wslError = $state<string | null>(null);
  wslToggles = new SvelteSet<WslToggle>();
  distroToInstall = $state<string | null>(null);

  dryRun = $state(true);
  running = $state(false);
  tasks = $state<TaskState[]>([]);
  summary = $state<RunSummary | null>(null);
  runError = $state<string | null>(null);

  isWindows = $derived(this.platform?.os === "windows");
  selectedApps = $derived(this.catalog.filter((app) => this.selected.has(app.id)));

  pendingWslActions = $derived.by((): WslAction[] => {
    const actions: WslAction[] = [];
    if (this.wslToggles.has("installWsl")) actions.push({ kind: "installWsl" });
    if (this.distroToInstall) actions.push({ kind: "installDistro", distro: this.distroToInstall });
    for (const kind of ["enableSystemd", "dockerEngine", "vscodeExtension"] as const) {
      if (this.wslToggles.has(kind)) actions.push({ kind });
    }
    return actions;
  });

  selectionCount = $derived(this.selected.size + this.pendingWslActions.length);

  targetOf(app: CatalogApp): Target {
    return this.targets.get(app.id) ?? "host";
  }

  resolutionFor(app: CatalogApp): Resolution | null {
    return this.targetOf(app) === "wsl" ? app.wslResolution : app.resolution;
  }

  isAvailable(app: CatalogApp): boolean {
    return this.resolutionFor(app)?.status === "available";
  }

  isInstalled(app: CatalogApp): boolean {
    return this.targetOf(app) === "wsl" ? this.installedWsl.has(app.id) : this.installed.has(app.id);
  }

  async load(): Promise<void> {
    try {
      const [platform, catalog] = await Promise.all([getPlatform(), getCatalog()]);
      this.platform = platform;
      this.catalog = catalog;
    } catch (error) {
      this.loadError = message(error);
      return;
    }
    await Promise.all([this.refreshInstalled(), this.refreshWsl()]);
  }

  async refreshInstalled(): Promise<void> {
    if (this.checkingInstalled) return;
    this.checkingInstalled = true;
    try {
      const ids = await getInstalled();
      this.installed.clear();
      for (const id of ids) this.installed.add(id);
    } catch (error) {
      console.warn("Couldn't check installed apps:", message(error));
    } finally {
      this.checkingInstalled = false;
    }
  }

  /** Re-reads WSL, then the catalog (WSL availability depends on the distro) and what's installed there. */
  async refreshWsl(distro?: string): Promise<void> {
    if (!this.isWindows || this.checkingWsl) return;
    this.checkingWsl = true;
    this.wslError = null;
    try {
      this.wsl = await getWslStatus(distro ?? this.wsl?.active?.name);
      this.catalog = await getCatalog();
      const ids = await getWslInstalled();
      this.installedWsl.clear();
      for (const id of ids) this.installedWsl.add(id);
      for (const app of this.selectedApps) {
        if (!this.isAvailable(app)) this.selected.delete(app.id);
      }
    } catch (error) {
      this.wslError = message(error);
    } finally {
      this.checkingWsl = false;
    }
  }

  setTarget(app: CatalogApp, target: Target): void {
    this.targets.set(app.id, target);
    if (this.selected.has(app.id) && !this.isAvailable(app)) this.selected.delete(app.id);
  }

  toggle(app: CatalogApp): void {
    if (!this.isAvailable(app)) return;
    if (this.selected.has(app.id)) this.selected.delete(app.id);
    else this.selected.add(app.id);
  }

  toggleWsl(kind: WslToggle): void {
    if (this.wslToggles.has(kind)) this.wslToggles.delete(kind);
    else this.wslToggles.add(kind);
  }

  /** Moves the dev tools into WSL and selects the ones not installed there yet. */
  selectDevOnWsl(): void {
    for (const app of this.catalog.filter((a) => DEV_ON_WSL.includes(a.id))) {
      this.targets.set(app.id, "wsl");
      if (this.isAvailable(app) && !this.installedWsl.has(app.id)) this.selected.add(app.id);
    }
  }

  clearSelection(): void {
    this.selected.clear();
    this.wslToggles.clear();
    this.distroToInstall = null;
  }

  resetRun(): void {
    this.tasks = [];
    this.summary = null;
    this.runError = null;
  }

  async run(): Promise<void> {
    if (this.running || this.selectionCount === 0) return;
    const dryRun = this.dryRun;
    const request: RunRequest = {
      apps: this.selectedApps.map((app) => ({ id: app.id, target: this.targetOf(app) })),
      wsl: this.pendingWslActions,
    };
    this.resetRun();
    this.running = true;
    try {
      this.summary = await startRun(request, dryRun, (event) => this.onEvent(event));
    } catch (error) {
      this.runError = message(error);
    } finally {
      this.running = false;
    }
    if (!dryRun) {
      this.wslToggles.clear();
      this.distroToInstall = null;
      await Promise.all([this.refreshInstalled(), this.refreshWsl()]);
    }
  }

  private onEvent(event: RunEvent): void {
    switch (event.event) {
      case "taskStarted":
        this.tasks.push({
          id: event.taskId,
          label: event.label,
          command: event.command,
          status: "running",
          exitCode: null,
          lines: [],
        });
        break;
      case "output": {
        const task = this.tasks.find((t) => t.id === event.taskId);
        if (!task) break;
        task.lines.push(event.line);
        if (task.lines.length > MAX_LINES_PER_TASK) task.lines.shift();
        break;
      }
      case "taskFinished": {
        const task = this.tasks.find((t) => t.id === event.taskId);
        if (!task) break;
        task.status = event.status;
        task.exitCode = event.exitCode;
        break;
      }
      case "done":
        break;
    }
  }
}

export const store = new AppStore();
