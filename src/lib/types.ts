// Mirrors the structs exposed by the Rust backend (src-tauri/src). Keep in sync.

export type Os = "windows" | "linux" | "other";
export type PmId = "winget" | "github" | "flatpak" | "apt";
export type Target = "host" | "wsl";
export type WingetSource = "winget" | "msstore";
export type Category =
  | "browsers"
  | "dev"
  | "communication"
  | "media"
  | "utilities"
  | "gaming"
  | "productivity"
  | "security";

// platform.rs
export interface Distro {
  id: string;
  name: string;
  idLike: string[];
}

export interface PlatformInfo {
  os: Os;
  arch: string;
  version: string;
  distro: Distro | null;
  managers: PmId[];
}

// catalog.rs
export type WingetPackage = string | { id: string; source: WingetSource };

export interface GithubPackage {
  repo: string;
  assetSuffix: string;
  installer: "msi";
  detectName: string;
}

export type FlatpakPackage = string | { id: string; remote: { name: string; url: string } };

export interface Packages {
  winget?: WingetPackage;
  github?: GithubPackage;
  flatpak?: FlatpakPackage;
  apt?: string;
}

// resolve.rs
export type Resolution =
  | { status: "available"; manager: PmId; package: string }
  | { status: "unavailable"; reason: string };

// commands.rs (CatalogEntry = App + resolution)
export interface CatalogApp {
  id: string;
  name: string;
  description: string;
  category: Category;
  packages: Packages;
  targets: Target[];
  preferLinux: "flatpak" | "native";
  resolution: Resolution;
  /** How the app installs inside WSL; null when it has no WSL target (or outside Windows). */
  wslResolution: Resolution | null;
}

// wsl.rs
export interface WslDistro {
  name: string;
  running: boolean;
  version: number;
  isDefault: boolean;
}

export interface DistroDetails {
  name: string;
  osName: string;
  user: string;
  hasUser: boolean;
  systemd: boolean;
  apt: boolean;
  docker: boolean;
}

export interface WslStatus {
  installed: boolean;
  version: string | null;
  distros: WslDistro[];
  dockerDesktop: boolean;
  active: DistroDetails | null;
  /** null when VS Code isn't installed. */
  vscodeExtension: boolean | null;
}

export interface OnlineDistro {
  name: string;
  friendlyName: string;
}

export interface WslConfig {
  memory: string | null;
  processors: number | null;
  swap: string | null;
}

export type WslAction =
  | { kind: "installWsl" }
  | { kind: "installDistro"; distro: string }
  | { kind: "enableSystemd" }
  | { kind: "dockerEngine" }
  | { kind: "vscodeExtension" };

// runner.rs
export interface AppRequest {
  id: string;
  target: Target;
}

/** What to run; also the selection stored in profiles and backups. */
export interface RunRequest {
  apps: AppRequest[];
  wsl: WslAction[];
}

// runner.rs ("done" is a setup step that finished, not an app install)
export type TaskStatus = "succeeded" | "done" | "alreadyInstalled" | "failed" | "simulated";

export type RunEvent =
  | { event: "taskStarted"; taskId: string; label: string; command: string }
  | { event: "output"; taskId: string; line: string }
  | { event: "taskFinished"; taskId: string; status: TaskStatus; exitCode: number | null }
  | { event: "done"; succeeded: number; failed: number };

export interface RunSummary {
  succeeded: number;
  failed: number;
}

// profiles.rs
export interface Profile {
  id: string;
  name: string;
  description: string;
  apps: AppRequest[];
  wsl: WslAction[];
}

export interface ProfileEntry extends Profile {
  /** Ships with In8 (can't be deleted); otherwise saved by the user. */
  builtin: boolean;
}

export interface ImportResult {
  selection: RunRequest;
  /** How many saved profiles the file brought in. */
  profiles: number;
  /** App IDs from the file that this version of In8 doesn't know. */
  unknownApps: string[];
}
