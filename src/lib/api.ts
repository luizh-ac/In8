// Single entry point for calls into the Rust backend.
import { Channel, invoke } from "@tauri-apps/api/core";
import type { CatalogApp, PlatformInfo, RunEvent, RunSummary } from "./types";

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

export function startInstall(
  appIds: string[],
  dryRun: boolean,
  onEvent: (event: RunEvent) => void,
): Promise<RunSummary> {
  const channel = new Channel<RunEvent>();
  channel.onmessage = onEvent;
  return invoke<RunSummary>("start_install", { appIds, dryRun, onEvent: channel });
}
