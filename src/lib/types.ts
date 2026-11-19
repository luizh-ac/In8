// Mirrors the structs exposed by the Rust backend (src-tauri/src). Keep in sync.

export type Os = "windows" | "linux" | "other";
export type PmId = "winget" | "flatpak";
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

export interface Packages {
  winget?: WingetPackage;
  flatpak?: string;
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
}

// runner.rs
export type TaskStatus = "succeeded" | "alreadyInstalled" | "failed" | "simulated";

export type RunEvent =
  | { event: "taskStarted"; taskId: string; label: string; command: string }
  | { event: "output"; taskId: string; line: string }
  | { event: "taskFinished"; taskId: string; status: TaskStatus; exitCode: number | null }
  | { event: "done"; succeeded: number; failed: number };

export interface RunSummary {
  succeeded: number;
  failed: number;
}
