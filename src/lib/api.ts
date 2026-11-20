// Single entry point for calls into the Rust backend.
import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AdminStatus,
  CatalogApp,
  DebloatInfo,
  Settings,
  TweakInfo,
  ImportResult,
  OnlineDistro,
  PlatformInfo,
  PostInstallInfo,
  ProfileEntry,
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

/** Built-in profiles first, then the user's saved ones. */
export function getProfiles(): Promise<ProfileEntry[]> {
  return invoke<ProfileEntry[]>("get_profiles");
}

/** Saves the selection as a profile (the same name replaces it); returns all profiles. */
export function saveProfile(name: string, selection: RunRequest): Promise<ProfileEntry[]> {
  return invoke<ProfileEntry[]>("save_profile", { name, selection });
}

export function deleteProfile(id: string): Promise<ProfileEntry[]> {
  return invoke<ProfileEntry[]>("delete_profile", { id });
}

/** Opens a save dialog and writes the backup. Resolves to the file path, or null if cancelled. */
export function exportBackup(selection: RunRequest): Promise<string | null> {
  return invoke<string | null>("export_backup", { selection });
}

/** Opens a file picker and imports a backup. Resolves to null if cancelled. */
export function importBackup(): Promise<ImportResult | null> {
  return invoke<ImportResult | null>("import_backup");
}

/** Tweaks for this system with their current state. */
export function getTweaks(): Promise<TweakInfo[]> {
  return invoke<TweakInfo[]>("get_tweaks");
}

/** Debloat items with whether they're installed (empty outside Windows). Takes a few seconds. */
export function getDebloat(): Promise<DebloatInfo[]> {
  return invoke<DebloatInfo[]>("get_debloat");
}

/** Post-install tasks plus what's already set up. Takes a few seconds (asks Git, VS Code and WSL). */
export function getPostinstall(): Promise<PostInstallInfo> {
  return invoke<PostInstallInfo>("get_postinstall");
}

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export function saveSettings(settings: Settings): Promise<Settings> {
  return invoke<Settings>("save_settings", { settings });
}

export function getAdminStatus(): Promise<AdminStatus> {
  return invoke<AdminStatus>("get_admin_status");
}

/** Relaunches In8 elevated with this selection. Resolves to false if UAC was declined (on success the window closes). */
export function restartAsAdmin(selection: RunRequest): Promise<boolean> {
  return invoke<boolean>("restart_as_admin", { selection });
}

/** The selection carried over by "Restart as administrator", if any. */
export function takePendingSelection(): Promise<RunRequest | null> {
  return invoke<RunRequest | null>("take_pending_selection");
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
