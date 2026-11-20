// App-wide state (Svelte 5 runes).
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import {
  deleteProfile,
  getCatalog,
  getInstalled,
  getPlatform,
  getProfiles,
  getWslInstalled,
  getWslStatus,
  saveProfile,
  startRun,
} from "./api";
import { wslActionLabel } from "./strings";
import type {
  CatalogApp,
  PlatformInfo,
  ProfileEntry,
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

/** What applying a profile or an imported selection did. */
export interface ApplyResult {
  /** Items newly added to the selection. */
  selected: number;
  /** Names of apps and WSL options left out because they're already there. */
  alreadyInstalled: string[];
  /** Names of apps that can't be installed on this system. */
  skipped: string[];
  /** Some WSL parts wait for WSL and a distro to be set up first. */
  needsWslSetup: boolean;
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

  profiles = $state<ProfileEntry[]>([]);
  profileError = $state<string | null>(null);

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
    await Promise.all([this.refreshInstalled(), this.refreshWsl(), this.loadProfiles()]);
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

  /** The current selection, in the shape runs, profiles and backups use. */
  currentSelection(): RunRequest {
    return {
      apps: this.selectedApps.map((app) => ({ id: app.id, target: this.targetOf(app) })),
      wsl: this.pendingWslActions,
    };
  }

  /**
   * Adds a profile or imported selection to the current one. Apps already installed stay unselected;
   * apps that can't be installed here are reported, not selected.
   */
  applySelection(selection: RunRequest): ApplyResult {
    const result: ApplyResult = { selected: 0, alreadyInstalled: [], skipped: [], needsWslSetup: false };
    const ready = this.wsl?.active ?? null;
    const distroReady = !!ready && ready.hasUser && ready.apt;

    for (const wanted of selection.apps) {
      const app = this.catalog.find((a) => a.id === wanted.id);
      if (!app) continue;
      // WSL only exists on Windows; elsewhere the app installs natively.
      const target: Target = this.isWindows && wanted.target === "wsl" ? "wsl" : "host";
      const resolution = target === "wsl" ? app.wslResolution : app.resolution;
      if (resolution?.status !== "available") {
        if (target === "wsl" && !distroReady) result.needsWslSetup = true;
        else result.skipped.push(app.name);
        continue;
      }
      const installed = target === "wsl" ? this.installedWsl.has(app.id) : this.installed.has(app.id);
      if (installed) {
        result.alreadyInstalled.push(app.name);
        continue;
      }
      this.targets.set(app.id, target);
      if (!this.selected.has(app.id)) result.selected++;
      this.selected.add(app.id);
    }

    if (!this.isWindows) return result;
    const status = this.wsl;
    const wantsWsl = selection.wsl.length > 0 || selection.apps.some((a) => a.target === "wsl");
    if (wantsWsl && status && !status.installed) {
      this.addWslToggle("installWsl", result);
      result.needsWslSetup = true;
      return result;
    }
    for (const action of selection.wsl) {
      switch (action.kind) {
        case "installWsl":
          if (status?.installed) result.alreadyInstalled.push(wslActionLabel(action));
          else this.addWslToggle("installWsl", result);
          break;
        case "installDistro":
          if (status?.distros.some((d) => d.name === action.distro)) {
            result.alreadyInstalled.push(wslActionLabel(action));
          } else if (this.distroToInstall !== action.distro) {
            this.distroToInstall = action.distro;
            result.selected++;
          }
          break;
        case "enableSystemd":
        case "dockerEngine": {
          const done = action.kind === "enableSystemd" ? ready?.systemd : ready?.docker;
          if (done) {
            result.alreadyInstalled.push(wslActionLabel(action));
          } else if (!distroReady) {
            result.needsWslSetup = true;
          } else {
            this.addWslToggle(action.kind, result);
            // Docker Engine needs systemd.
            if (action.kind === "dockerEngine" && !ready?.systemd) this.addWslToggle("enableSystemd", result);
          }
          break;
        }
        case "vscodeExtension":
          if (status?.vscodeExtension === true) result.alreadyInstalled.push(wslActionLabel(action));
          else this.addWslToggle("vscodeExtension", result);
          break;
      }
    }
    return result;
  }

  private addWslToggle(kind: WslToggle, result: ApplyResult): void {
    if (this.wslToggles.has(kind)) return;
    this.wslToggles.add(kind);
    result.selected++;
  }

  /** Selects everything installed on this PC (and in WSL), to save it as a profile or export it. */
  selectInstalled(): number {
    const before = this.selectionCount;
    for (const app of this.catalog) {
      if (this.installedWsl.has(app.id) && app.wslResolution?.status === "available") {
        this.targets.set(app.id, "wsl");
        this.selected.add(app.id);
      } else if (this.installed.has(app.id) && app.resolution.status === "available") {
        this.targets.set(app.id, "host");
        this.selected.add(app.id);
      }
    }
    const active = this.wsl?.active;
    if (active?.systemd) this.wslToggles.add("enableSystemd");
    if (active?.docker) this.wslToggles.add("dockerEngine");
    if (this.wsl?.vscodeExtension === true) this.wslToggles.add("vscodeExtension");
    return this.selectionCount - before;
  }

  async loadProfiles(): Promise<void> {
    try {
      this.profiles = await getProfiles();
      this.profileError = null;
    } catch (error) {
      this.profileError = message(error);
    }
  }

  async saveProfile(name: string): Promise<void> {
    this.profiles = await saveProfile(name, this.currentSelection());
  }

  async deleteProfile(id: string): Promise<void> {
    this.profiles = await deleteProfile(id);
  }

  async run(): Promise<void> {
    if (this.running || this.selectionCount === 0) return;
    const dryRun = this.dryRun;
    const request = this.currentSelection();
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
