// Single entry point for calls into the Rust backend.
import { invoke } from "@tauri-apps/api/core";
import type { PlatformInfo } from "./types";

export function getPlatform(): Promise<PlatformInfo> {
  return invoke<PlatformInfo>("get_platform");
}
