// App-wide state (Svelte 5 runes).
import { SvelteSet } from "svelte/reactivity";
import { getCatalog, getInstalled, getPlatform, startInstall } from "./api";
import type { CatalogApp, PlatformInfo, RunEvent, RunSummary, TaskStatus } from "./types";

export type View =
  | "apps"
  | "profiles"
  | "wsl"
  | "debloat"
  | "tweaks"
  | "postinstall"
  | "settings"
  | "run";

export interface TaskState {
  id: string;
  label: string;
  command: string;
  status: TaskStatus | "running";
  exitCode: number | null;
  lines: string[];
}

const MAX_LINES_PER_TASK = 500;

function message(error: unknown): string {
  return typeof error === "string" ? error : error instanceof Error ? error.message : String(error);
}

class AppStore {
  view = $state<View>("apps");
  platform = $state<PlatformInfo | null>(null);
  catalog = $state<CatalogApp[]>([]);
  loadError = $state<string | null>(null);

  installed = new SvelteSet<string>();
  checkingInstalled = $state(false);
  selected = new SvelteSet<string>();

  dryRun = $state(true);
  running = $state(false);
  tasks = $state<TaskState[]>([]);
  summary = $state<RunSummary | null>(null);
  runError = $state<string | null>(null);

  selectedApps = $derived(this.catalog.filter((app) => this.selected.has(app.id)));

  async load(): Promise<void> {
    try {
      const [platform, catalog] = await Promise.all([getPlatform(), getCatalog()]);
      this.platform = platform;
      this.catalog = catalog;
    } catch (error) {
      this.loadError = message(error);
      return;
    }
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

  toggle(app: CatalogApp): void {
    if (app.resolution.status !== "available") return;
    if (this.selected.has(app.id)) this.selected.delete(app.id);
    else this.selected.add(app.id);
  }

  clearSelection(): void {
    this.selected.clear();
  }

  resetRun(): void {
    this.tasks = [];
    this.summary = null;
    this.runError = null;
  }

  async run(): Promise<void> {
    if (this.running || this.selected.size === 0) return;
    const dryRun = this.dryRun;
    this.resetRun();
    this.running = true;
    try {
      this.summary = await startInstall([...this.selected], dryRun, (event) => this.onEvent(event));
    } catch (error) {
      this.runError = message(error);
    } finally {
      this.running = false;
    }
    if (!dryRun) await this.refreshInstalled();
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
