// App-wide state (Svelte 5 runes).
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import {
  deleteProfile,
  checkUpdate,
  getAdminStatus,
  getCatalog,
  getDebloat,
  getInstalled,
  getPlatform,
  getPostinstall,
  getProfiles,
  getSettings,
  getTweaks,
  getWslInstalled,
  getWslStatus,
  installUpdate,
  refreshPlatform,
  restartAsAdmin,
  saveProfile,
  saveSettings,
  startRun,
  takePendingSelection,
} from "./api";
import { setLanguage, systemLanguage, wslActionLabel } from "./strings.svelte";
import type {
  AdminStatus,
  CatalogApp,
  DebloatInfo,
  PlatformInfo,
  PostInstallInfo,
  PostTask,
  Profile,
  ProfileEntry,
  Resolution,
  RunEvent,
  RunRequest,
  RunSummary,
  Settings,
  Target,
  TaskStatus,
  TweakInfo,
  UpdateInfo,
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

export type TweakChoice = "apply" | "revert";

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
  /** Names of apps, tweaks and WSL options left out because they're already there. */
  alreadyInstalled: string[];
  /** Names of apps that can't be installed on this system. */
  skipped: string[];
  /** Some WSL parts wait for WSL and a distro to be set up first. */
  needsWslSetup: boolean;
}

const MAX_LINES_PER_TASK = 500;
/** Apps the "Dev tools on WSL" shortcut moves into WSL. */
export const DEV_ON_WSL = ["git", "nodejs", "python", "rust"];

const DEFAULT_SETTINGS: Settings = {
  alwaysAdmin: false,
  dryRunByDefault: true,
  theme: "system",
  language: "system",
  wslDistro: null,
  linuxPreferNative: false,
};

export function message(error: unknown): string {
  return typeof error === "string" ? error : error instanceof Error ? error.message : String(error);
}

/** A profile in the shape of a selection (profiles only ever apply tweaks, never revert). */
export function profileSelection(profile: Profile): RunRequest {
  return {
    apps: profile.apps,
    wsl: profile.wsl,
    tweaks: profile.tweaks.map((id) => ({ id, revert: false })),
    debloat: profile.debloat,
    postinstall: profile.postinstall,
    setupFlatpak: false,
    repairWinget: false,
  };
}

/** Key of a chosen post-install task: one task can run on Windows and in WSL. */
export function postKey(id: string, target: Target): string {
  return `${id}@${target}`;
}

class AppStore {
  view = $state<View>("apps");
  /** Where "Back" from the run screen returns to. */
  previousView = $state<View>("apps");
  platform = $state<PlatformInfo | null>(null);
  catalog = $state<CatalogApp[]>([]);
  loadError = $state<string | null>(null);

  settings = $state<Settings>({ ...DEFAULT_SETTINGS });
  admin = $state<AdminStatus>({ elevated: false, declined: false, canElevate: false });

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

  tweaks = $state<TweakInfo[]>([]);
  checkingTweaks = $state(false);
  tweakError = $state<string | null>(null);
  tweakChoices = new SvelteMap<string, TweakChoice>();

  debloatItems = $state<DebloatInfo[]>([]);
  checkingDebloat = $state(false);
  debloatError = $state<string | null>(null);
  debloatSelected = new SvelteSet<string>();

  postTasks = $state<PostTask[]>([]);
  postState = $state<PostInstallInfo["state"] | null>(null);
  checkingPost = $state(false);
  postError = $state<string | null>(null);
  /** Chosen post-install tasks, as postKey(id, target). */
  postSelected = new SvelteSet<string>();
  /** Typed values per task ID (shared by its Windows and WSL runs). */
  postValues = new SvelteMap<string, Record<string, string>>();
  /** Picked options per task ID (e.g. VS Code extensions). */
  postOptions = new SvelteMap<string, string[]>();

  /** Linux: install Flatpak in the next run. */
  setupFlatpak = $state(false);
  /** Windows: register winget in the next run. */
  repairWinget = $state(false);

  profiles = $state<ProfileEntry[]>([]);
  profileError = $state<string | null>(null);

  update = $state<UpdateInfo | null>(null);
  checkingUpdate = $state(false);
  installingUpdate = $state(false);
  updateError = $state<string | null>(null);
  /** The last check found nothing newer. */
  upToDate = $state(false);

  dryRun = $state(true);
  running = $state(false);
  tasks = $state<TaskState[]>([]);
  summary = $state<RunSummary | null>(null);
  runError = $state<string | null>(null);

  isWindows = $derived(this.platform?.os === "windows");
  selectedApps = $derived(this.catalog.filter((app) => this.selected.has(app.id)));
  selectedTweaks = $derived(this.tweaks.filter((tweak) => this.tweakChoices.has(tweak.id)));
  selectedDebloat = $derived(this.debloatItems.filter((item) => this.debloatSelected.has(item.id)));

  pendingWslActions = $derived.by((): WslAction[] => {
    const actions: WslAction[] = [];
    if (this.wslToggles.has("installWsl")) actions.push({ kind: "installWsl" });
    if (this.distroToInstall) actions.push({ kind: "installDistro", distro: this.distroToInstall });
    for (const kind of ["enableSystemd", "dockerEngine", "vscodeExtension"] as const) {
      if (this.wslToggles.has(kind)) actions.push({ kind });
    }
    return actions;
  });

  selectionCount = $derived(
    this.selected.size +
      this.pendingWslActions.length +
      this.tweakChoices.size +
      this.debloatSelected.size +
      this.postSelected.size +
      (this.setupFlatpak ? 1 : 0) +
      (this.repairWinget ? 1 : 0),
  );

  /** A selected tweak writes to HKLM and In8 isn't elevated (Windows only). */
  needsAdmin = $derived(
    this.admin.canElevate && !this.admin.elevated && this.selectedTweaks.some((tweak) => tweak.requiresAdmin),
  );

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
      const [platform, catalog, settings, admin] = await Promise.all([
        getPlatform(),
        getCatalog(),
        getSettings(),
        getAdminStatus(),
      ]);
      this.platform = platform;
      this.catalog = catalog;
      this.settings = settings;
      this.admin = admin;
      this.dryRun = settings.dryRunByDefault;
    } catch (error) {
      this.loadError = message(error);
      return;
    }
    this.applyTheme();
    this.applyLanguage();
    window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => this.applyTheme());
    await Promise.all([
      this.refreshInstalled(),
      // Post-install reads the distro, so it waits for WSL.
      this.refreshWsl(this.settings.wslDistro ?? undefined).then(() => this.refreshPostinstall()),
      this.loadProfiles(),
      this.refreshTweaks(),
      this.refreshDebloat(),
    ]);
    // Quietly look for a newer In8; problems only show when checking from Settings.
    void this.checkForUpdate(true);
    // Coming back from "Restart as administrator": restore the selection and go straight to review.
    const pending = await takePendingSelection().catch(() => null);
    if (pending) {
      this.restoreSelection(pending);
      this.previousView = "apps";
      this.view = "run";
    }
  }

  applyTheme(): void {
    const theme = this.settings.theme;
    const dark =
      theme === "dark" || (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
    document.documentElement.classList.toggle("dark", dark);
    document.documentElement.style.colorScheme = dark ? "dark" : "light";
  }

  applyLanguage(): void {
    const language = this.settings.language;
    setLanguage(language === "system" ? systemLanguage() : language);
  }

  async updateSettings(patch: Partial<Settings>): Promise<void> {
    const preferNativeChanged =
      patch.linuxPreferNative !== undefined && patch.linuxPreferNative !== this.settings.linuxPreferNative;
    this.settings = await saveSettings({ ...this.settings, ...patch });
    this.applyTheme();
    this.applyLanguage();
    if (preferNativeChanged) await this.reloadPlatform();
  }

  /** Detects package managers again, then reloads the catalog and what's installed (resolutions change). */
  async reloadPlatform(): Promise<void> {
    this.platform = await refreshPlatform();
    this.catalog = await getCatalog();
    for (const app of this.selectedApps) {
      if (!this.isAvailable(app)) this.selected.delete(app.id);
    }
    if (this.setupFlatpak && this.platform.managers.includes("flatpak")) this.setupFlatpak = false;
    if (this.repairWinget && this.platform.managers.includes("winget")) this.repairWinget = false;
    await this.refreshInstalled();
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
      // Remember the distro the user works with.
      const active = this.wsl?.active?.name ?? null;
      if (distro && active === distro && this.settings.wslDistro !== distro) {
        await this.updateSettings({ wslDistro: distro }).catch(() => {});
      }
    } catch (error) {
      this.wslError = message(error);
    } finally {
      this.checkingWsl = false;
    }
  }

  async refreshTweaks(): Promise<void> {
    if (this.checkingTweaks) return;
    this.checkingTweaks = true;
    try {
      this.tweaks = await getTweaks();
      this.tweakError = null;
      for (const [id, choice] of this.tweakChoices) {
        const tweak = this.tweaks.find((t) => t.id === id);
        if (!tweak || (choice === "revert" && !tweak.revertible)) this.tweakChoices.delete(id);
      }
    } catch (error) {
      this.tweakError = message(error);
    } finally {
      this.checkingTweaks = false;
    }
  }

  async refreshDebloat(): Promise<void> {
    if (!this.isWindows || this.checkingDebloat) return;
    this.checkingDebloat = true;
    try {
      this.debloatItems = await getDebloat();
      this.debloatError = null;
      for (const id of this.debloatSelected) {
        if (!this.debloatItems.some((item) => item.id === id && item.installed)) this.debloatSelected.delete(id);
      }
    } catch (error) {
      this.debloatError = message(error);
    } finally {
      this.checkingDebloat = false;
    }
  }

  async refreshPostinstall(): Promise<void> {
    if (this.checkingPost) return;
    this.checkingPost = true;
    try {
      const info = await getPostinstall();
      this.postTasks = info.tasks;
      this.postState = info.state;
      this.postError = null;
      this.prefillPostValues();
      for (const key of this.postSelected) {
        const [id, target] = key.split("@") as [string, Target];
        if (!this.postTargets(id).includes(target)) this.postSelected.delete(key);
      }
    } catch (error) {
      this.postError = message(error);
    } finally {
      this.checkingPost = false;
    }
  }

  /** Defaults for tasks nothing was typed into yet: input defaults, current Git identity, every option. */
  private prefillPostValues(): void {
    const sides = [this.postState?.host, this.postState?.wsl].filter((side) => side != null);
    const gitName = sides.find((side) => side.gitName)?.gitName ?? "";
    const gitEmail = sides.find((side) => side.gitEmail)?.gitEmail ?? "";
    for (const task of this.postTasks) {
      if (!this.postValues.has(task.id)) {
        const values: Record<string, string> = {};
        for (const input of task.inputs) {
          values[input.id] =
            input.kind === "email" ? gitEmail : task.id === "git-identity" && input.id === "name" ? gitName : input.default;
        }
        this.postValues.set(task.id, values);
      }
      if (task.options.length > 0 && !this.postOptions.has(task.id)) {
        this.postOptions.set(
          task.id,
          task.options.map((option) => option.id),
        );
      }
    }
  }

  /** Where a post-install task can run here: WSL only on Windows with a ready distro. */
  postTargets(id: string): Target[] {
    const task = this.postTasks.find((t) => t.id === id);
    if (!task) return [];
    const active = this.wsl?.active;
    const wslReady = this.isWindows && !!active && active.hasUser && active.apt;
    return task.targets.filter((target) => target === "host" || wslReady);
  }

  togglePost(id: string, target: Target): void {
    const key = postKey(id, target);
    if (this.postSelected.has(key)) this.postSelected.delete(key);
    else this.postSelected.add(key);
  }

  setPostValue(id: string, input: string, value: string): void {
    this.postValues.set(id, { ...(this.postValues.get(id) ?? {}), [input]: value });
  }

  togglePostOption(id: string, option: string): void {
    const current = this.postOptions.get(id) ?? [];
    this.postOptions.set(
      id,
      current.includes(option) ? current.filter((o) => o !== option) : [...current, option],
    );
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

  /** Picks apply or revert for a tweak; choosing the current choice again clears it. */
  chooseTweak(id: string, choice: TweakChoice): void {
    if (this.tweakChoices.get(id) === choice) this.tweakChoices.delete(id);
    else this.tweakChoices.set(id, choice);
  }

  toggleDebloat(id: string): void {
    if (this.debloatSelected.has(id)) this.debloatSelected.delete(id);
    else this.debloatSelected.add(id);
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
    this.tweakChoices.clear();
    this.debloatSelected.clear();
    this.postSelected.clear();
    this.setupFlatpak = false;
    this.repairWinget = false;
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
      tweaks: [...this.tweakChoices].map(([id, choice]) => ({ id, revert: choice === "revert" })),
      debloat: [...this.debloatSelected],
      postinstall: [...this.postSelected].map((key) => {
        const [id, target] = key.split("@") as [string, Target];
        return {
          id,
          target,
          values: { ...(this.postValues.get(id) ?? {}) },
          options: [...(this.postOptions.get(id) ?? [])],
        };
      }),
      setupFlatpak: this.setupFlatpak,
      repairWinget: this.repairWinget,
    };
  }

  /** Puts back a selection exactly as it was (after "Restart as administrator"). */
  private restoreSelection(selection: RunRequest): void {
    this.clearSelection();
    for (const wanted of selection.apps) {
      const app = this.catalog.find((a) => a.id === wanted.id);
      if (!app) continue;
      this.targets.set(app.id, wanted.target);
      if (this.isAvailable(app)) this.selected.add(app.id);
    }
    for (const action of selection.wsl) {
      if (action.kind === "installDistro") this.distroToInstall = action.distro;
      else this.wslToggles.add(action.kind);
    }
    for (const wanted of selection.tweaks) {
      if (this.tweaks.some((t) => t.id === wanted.id)) {
        this.tweakChoices.set(wanted.id, wanted.revert ? "revert" : "apply");
      }
    }
    for (const id of selection.debloat) {
      if (this.debloatItems.some((item) => item.id === id && item.installed)) this.debloatSelected.add(id);
    }
    this.setupFlatpak = selection.setupFlatpak;
    this.repairWinget = selection.repairWinget;
    for (const wanted of selection.postinstall) {
      this.restorePostValues(wanted.id, wanted.values, wanted.options);
      if (this.postTargets(wanted.id).includes(wanted.target)) this.postSelected.add(postKey(wanted.id, wanted.target));
    }
  }

  private restorePostValues(id: string, values: Record<string, string>, options: string[]): void {
    if (Object.keys(values).length > 0) this.postValues.set(id, { ...(this.postValues.get(id) ?? {}), ...values });
    if (options.length > 0) this.postOptions.set(id, [...options]);
  }

  /**
   * Adds a profile or imported selection to the current one. Apps already installed and tweaks already
   * applied stay unselected; apps that can't be installed here are reported, not selected.
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

    // Tweaks for another OS aren't in `this.tweaks` and are left out quietly.
    for (const wanted of selection.tweaks) {
      const tweak = this.tweaks.find((t) => t.id === wanted.id);
      if (!tweak || wanted.revert) continue;
      if (tweak.applied) {
        result.alreadyInstalled.push(tweak.name);
        continue;
      }
      if (this.tweakChoices.get(tweak.id) !== "apply") result.selected++;
      this.tweakChoices.set(tweak.id, "apply");
    }
    // Items that aren't installed have nothing to remove.
    for (const id of selection.debloat) {
      const item = this.debloatItems.find((d) => d.id === id);
      if (!item?.installed || this.debloatSelected.has(id)) continue;
      this.debloatSelected.add(id);
      result.selected++;
    }

    for (const wanted of selection.postinstall) {
      const task = this.postTasks.find((t) => t.id === wanted.id);
      if (!task) continue;
      // Without WSL, tasks that also run on the host run there instead.
      let target: Target = wanted.target;
      if (target === "wsl" && !this.isWindows) {
        if (!task.targets.includes("host")) continue;
        target = "host";
      }
      if (!this.postTargets(task.id).includes(target)) {
        if (target === "wsl") result.needsWslSetup = true;
        continue;
      }
      this.restorePostValues(task.id, wanted.values, wanted.options);
      const key = postKey(task.id, target);
      if (!this.postSelected.has(key)) result.selected++;
      this.postSelected.add(key);
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

  /**
   * Selects everything installed on this PC (and in WSL) and every tweak already applied,
   * to save it as a profile or export it.
   */
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
    for (const tweak of this.tweaks) {
      if (tweak.applied) this.tweakChoices.set(tweak.id, "apply");
    }
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

  async checkForUpdate(quiet = false): Promise<void> {
    if (this.checkingUpdate) return;
    this.checkingUpdate = true;
    this.updateError = null;
    try {
      this.update = await checkUpdate();
      this.upToDate = this.update === null;
    } catch (error) {
      if (!quiet) this.updateError = message(error);
    } finally {
      this.checkingUpdate = false;
    }
  }

  /** Downloads and installs the update; In8 restarts when it's done. */
  async installUpdate(): Promise<void> {
    if (this.installingUpdate || this.running) return;
    this.installingUpdate = true;
    this.updateError = null;
    try {
      await installUpdate();
    } catch (error) {
      this.updateError = message(error);
    } finally {
      this.installingUpdate = false;
    }
  }

  /** Relaunches elevated with the current selection. Resolves to false if UAC was declined. */
  restartAsAdmin(): Promise<boolean> {
    return restartAsAdmin(this.currentSelection());
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
      this.tweakChoices.clear();
      this.debloatSelected.clear();
      this.postSelected.clear();
      const managersChanged = this.setupFlatpak || this.repairWinget;
      this.setupFlatpak = false;
      this.repairWinget = false;
      // A run can add Flatpak, winget or the distro's packages; detect managers again first.
      if (this.platform?.os === "linux" || managersChanged) await this.reloadPlatform().catch(() => {});
      await Promise.all([
        this.refreshInstalled(),
        this.refreshWsl().then(() => this.refreshPostinstall()),
        this.refreshTweaks(),
        this.refreshDebloat(),
      ]);
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
