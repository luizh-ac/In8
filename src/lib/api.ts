// Single entry point for calls into the Rust backend.
import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  CatalogApp,
  OnlineDistro,
  PlatformInfo,
  RunEvent,
  RunRequest,
  RunSummary,
  WslConfig,
  WslStatus,
} from "./types";

export function getPlatform(): Promise<PlatformInfo> {
  return invoke<PlatformInfo>("get_platform");
}

export function getCatalog(): Promise<CatalogApp[]> {
  return invoke<CatalogApp[]>("get_catalog");
}

/** IDs of catalog apps already installed. Can take a few seconds. */
export function getInstalled(): Promise<string[]> {
  return invoke<string[]>("get_installed");
}

/** WSL setup and active distro (null outside Windows). Can take a second or two. */
export function getWslStatus(distro?: string): Promise<WslStatus | null> {
  return invoke<WslStatus | null>("get_wsl_status", { distro: distro ?? null });
}

export function getOnlineDistros(): Promise<OnlineDistro[]> {
  return invoke<OnlineDistro[]>("get_online_distros");
}

/** IDs of catalog apps installed inside the active WSL distro. */
export function getWslInstalled(): Promise<string[]> {
  return invoke<string[]>("get_wsl_installed");
}

export function openDistroTerminal(distro: string): Promise<void> {
  return invoke<void>("open_distro_terminal", { distro });
}

export function getWslConfig(): Promise<WslConfig> {
  return invoke<WslConfig>("get_wslconfig");
}

export function previewWslConfig(config: WslConfig): Promise<string> {
  return invoke<string>("preview_wslconfig", { config });
}

export function saveWslConfig(config: WslConfig): Promise<void> {
  return invoke<void>("save_wslconfig", { config });
}

export function startRun(
  request: RunRequest,
  dryRun: boolean,
  onEvent: (event: RunEvent) => void,
): Promise<RunSummary> {
  const channel = new Channel<RunEvent>();
  channel.onmessage = onEvent;
  return invoke<RunSummary>("start_run", { request, dryRun, onEvent: channel });
}
