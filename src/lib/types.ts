// Mirrors the structs exposed by the Rust backend (src-tauri/src). Keep in sync.

export type Os = "windows" | "linux" | "other";
export type PmId = "winget" | "github" | "flatpak" | "apt" | "dnf" | "pacman";
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
  /** Linux: distro packages are tried before Flatpak. */
  preferNative: boolean;
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
  dnf?: string;
  pacman?: string;
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

export interface TweakRequest {
  id: string;
  /** Restore the values saved when the tweak was applied. */
  revert: boolean;
}

/** What to run; also the selection stored in backups. */
export interface RunRequest {
  apps: AppRequest[];
  wsl: WslAction[];
  tweaks: TweakRequest[];
  /** Debloat item IDs to remove. */
  debloat: string[];
  postinstall: PostInstallRequest[];
  /** Linux: install Flatpak from the distro and add Flathub. */
  setupFlatpak: boolean;
  /** Windows: register App Installer so winget works. */
  repairWinget: boolean;
}

// runner.rs ("done": a setup step or tweak finished; "removed": a debloat item was uninstalled)
export type TaskStatus = "succeeded" | "done" | "removed" | "alreadyInstalled" | "failed" | "skipped" | "simulated";

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
  /** Tweak IDs to apply. */
  tweaks: string[];
  /** Debloat item IDs to remove. */
  debloat: string[];
  postinstall: PostInstallRequest[];
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

// tweaks.rs
export type TweakCategory = "appearance" | "explorer" | "taskbar" | "privacy" | "performance";

export type TweakAction =
  | { type: "registry"; hive: "HKCU" | "HKLM"; key: string; value: string; data: { dword: number } | { string: string } }
  | { type: "powerPlan"; scheme: string }
  | { type: "gsettings"; schema: string; key: string; value: string };

export interface Tweak {
  id: string;
  os: "windows" | "linux";
  desktop?: string;
  category: TweakCategory;
  name: string;
  description: string;
  /** Shows up fully only after signing out. */
  signOut: boolean;
  actions: TweakAction[];
}

export interface TweakInfo extends Tweak {
  requiresAdmin: boolean;
  /** Every value already matches the tweak. */
  applied: boolean;
  /** In8 saved the previous values, so it can revert. */
  revertible: boolean;
}

// debloat.rs
export interface DebloatItem {
  id: string;
  name: string;
  description: string;
  /** "advanced" items can break something someone uses; the run screen asks first. */
  level: "safe" | "advanced";
  appx?: string[];
  winget?: string;
}

export interface DebloatInfo extends DebloatItem {
  installed: boolean;
}

// settings.rs
export type Theme = "system" | "light" | "dark";
export type LanguageSetting = "system" | "en" | "pt-BR";

export interface Settings {
  /** Windows only: relaunch through UAC on start. */
  alwaysAdmin: boolean;
  dryRunByDefault: boolean;
  theme: Theme;
  language: LanguageSetting;
  wslDistro: string | null;
  /** Linux: install from the distro's packages before Flatpak. */
  linuxPreferNative: boolean;
}

// update.rs
export interface UpdateInfo {
  version: string;
  currentVersion: string;
  notes: string | null;
}

// commands.rs
export interface AdminStatus {
  elevated: boolean;
  /** "Always run as administrator" is on but UAC was declined at start. */
  declined: boolean;
  canElevate: boolean;
}

// postinstall.rs
export type InputKind = "text" | "email" | "gitUrl" | "folder";

export interface PostInput {
  id: string;
  label: string;
  kind: InputKind;
  default: string;
  placeholder: string;
}

export interface PostTask {
  id: string;
  name: string;
  description: string;
  targets: Target[];
  /** Catalog app the task needs (installed or selected for the same target). */
  requires?: string;
  inputs: PostInput[];
  /** Things to pick from, e.g. VS Code extensions. */
  options: { id: string; label: string }[];
  actions: { type: string }[];
}

export interface PostInstallRequest {
  id: string;
  target: Target;
  values: Record<string, string>;
  options: string[];
}

export interface PostSideState {
  gitName: string | null;
  gitEmail: string | null;
  /** Contents of ~/.ssh/id_ed25519.pub. */
  sshPublicKey: string | null;
  /** Installed VS Code extension IDs, lowercase. */
  extensions: string[];
}

export interface PostInstallInfo {
  tasks: PostTask[];
  state: { host: PostSideState; wsl: PostSideState | null };
}
