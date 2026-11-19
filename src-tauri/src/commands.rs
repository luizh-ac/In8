//! Commands exposed to the frontend. Keep them thin: OS-specific logic lives in its own modules.

use serde::Serialize;

/// Mirrored in `src/lib/types.ts` (`PlatformInfo`).
#[derive(Debug, Serialize)]
pub struct PlatformInfo {
    pub os: &'static str,
    pub arch: &'static str,
    pub version: &'static str,
}

#[tauri::command]
pub fn get_platform() -> PlatformInfo {
    PlatformInfo {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        version: env!("CARGO_PKG_VERSION"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_info_reports_current_target() {
        let info = get_platform();
        assert_eq!(info.os, std::env::consts::OS);
        assert!(!info.version.is_empty());
    }
}
