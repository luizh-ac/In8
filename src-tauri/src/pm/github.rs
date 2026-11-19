//! Installs Windows apps that aren't in winget from their GitHub releases.
//! The installer is only run after its SHA-256 matches the digest GitHub publishes for it.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::InstallOutcome;
use crate::catalog::{GithubPackage, InstallerKind};
use crate::exec::Cmd;

const API: &str = "https://api.github.com/repos";
const DOWNLOAD_HOST: &str = "https://github.com/";
const USER_AGENT: &str = concat!("In8/", env!("CARGO_PKG_VERSION"));
const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// msiexec: ERROR_SUCCESS_REBOOT_REQUIRED, ERROR_SUCCESS_REBOOT_INITIATED, ERROR_PRODUCT_VERSION.
const MSI_REBOOT_REQUIRED: i32 = 3010;
const MSI_REBOOT_INITIATED: i32 = 1641;
const MSI_NEWER_VERSION_INSTALLED: i32 = 1638;

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

/// A downloaded installer, deleted together with its folder when dropped.
pub struct Download {
    dir: PathBuf,
    pub path: PathBuf,
}

impl Drop for Download {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The asset suffix with `{arch}` replaced for this machine.
pub fn asset_suffix(package: &GithubPackage) -> String {
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        _ => "x64",
    };
    package.asset_suffix.replace("{arch}", arch)
}

/// Shown in dry runs and at the start of the task.
pub fn describe(package: &GithubPackage) -> String {
    format!(
        "download the latest *{} from github.com/{}, check its SHA-256, then run {}",
        asset_suffix(package),
        package.repo,
        installer_cmd(package, Path::new("<file>")).display()
    )
}

pub fn installer_cmd(package: &GithubPackage, file: &Path) -> Cmd {
    match package.installer {
        InstallerKind::Msi => {
            Cmd::new("msiexec").args(["/i", &file.to_string_lossy(), "/passive", "/norestart"])
        }
    }
}

pub fn interpret_exit(code: Option<i32>) -> InstallOutcome {
    match code {
        Some(0 | MSI_REBOOT_REQUIRED | MSI_REBOOT_INITIATED) => InstallOutcome::Installed,
        Some(MSI_NEWER_VERSION_INSTALLED) => InstallOutcome::AlreadyInstalled,
        _ => InstallOutcome::Failed,
    }
}

/// Whether the app shows up in Windows "Installed apps" under its exact name.
pub fn is_installed(package: &GithubPackage) -> bool {
    Cmd::new("winget")
        .args([
            "list",
            "--name",
            &package.detect_name,
            "--exact",
            "--accept-source-agreements",
            "--disable-interactivity",
        ])
        .to_command()
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Downloads the newest matching installer into a fresh temporary folder and verifies it.
pub fn download(package: &GithubPackage, mut log: impl FnMut(String)) -> Result<Download, String> {
    let release: Release = serde_json::from_str(&http_get_text(&format!(
        "{API}/{}/releases/latest",
        package.repo
    ))?)
    .map_err(|e| format!("Unexpected answer from GitHub: {e}"))?;
    let asset = select_asset(&release, &asset_suffix(package))?;
    let expected = expected_sha256(asset)?;

    log(format!(
        "Downloading {} ({:.1} MB) from release {}",
        asset.name,
        asset.size as f64 / 1_048_576.0,
        release.tag_name
    ));
    let dir = std::env::temp_dir().join(format!("in8-download-{}", std::process::id()));
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Couldn't create a temporary folder: {e}"))?;
    let download = Download {
        path: dir.join(&asset.name),
        dir,
    };

    let actual = save_and_hash(
        &asset.browser_download_url,
        asset.size,
        &download.path,
        &mut log,
    )?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(format!(
            "SHA-256 mismatch: expected {expected}, got {actual}. The file was discarded."
        ));
    }
    log("SHA-256 verified".to_string());
    Ok(download)
}

fn select_asset<'a>(release: &'a Release, suffix: &str) -> Result<&'a Asset, String> {
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name.ends_with(suffix))
        .ok_or_else(|| {
            format!(
                "Release {} has no file ending in {suffix}",
                release.tag_name
            )
        })?;
    let safe_name = !asset.name.contains(['/', '\\']) && !asset.name.contains("..");
    if !safe_name {
        return Err(format!(
            "Refusing a file with an unsafe name: {}",
            asset.name
        ));
    }
    if !asset.browser_download_url.starts_with(DOWNLOAD_HOST) {
        return Err(format!(
            "Refusing a download outside github.com: {}",
            asset.browser_download_url
        ));
    }
    Ok(asset)
}

fn expected_sha256(asset: &Asset) -> Result<&str, String> {
    asset
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .filter(|hex| hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| {
            format!(
                "GitHub didn't publish a SHA-256 for {}; refusing to install it",
                asset.name
            )
        })
}

fn http_get_text(url: &str) -> Result<String, String> {
    ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Couldn't reach GitHub: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Couldn't read GitHub's answer: {e}"))
}

/// Streams the file to disk while hashing it, logging progress every 10%.
fn save_and_hash(
    url: &str,
    size: u64,
    path: &Path,
    log: &mut impl FnMut(String),
) -> Result<String, String> {
    let mut response = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD_BYTES)
        .reader();
    let mut file = File::create(path).map_err(|e| format!("Couldn't save the download: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let (mut written, mut next_report) = (0u64, 10u64);
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|e| format!("Download failed: {e}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read])
            .map_err(|e| format!("Couldn't save the download: {e}"))?;
        written += read as u64;
        if size > 0 && written * 100 / size >= next_report {
            log(format!("Downloaded {next_report}%"));
            next_report += 10;
        }
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> GithubPackage {
        GithubPackage {
            repo: "storytold/craft-launcher".into(),
            asset_suffix: "-windows-{arch}.msi".into(),
            installer: InstallerKind::Msi,
            detect_name: "ArtCraft Launcher".into(),
        }
    }

    fn asset(name: &str, url: &str, digest: Option<&str>) -> Asset {
        Asset {
            name: name.into(),
            browser_download_url: url.into(),
            size: 1,
            digest: digest.map(str::to_string),
        }
    }

    const GOOD_DIGEST: &str =
        "sha256:fa73b211cf0a7c377f35fc998582eaa7c9f62013a1236ee4fd16204669b5bbfa";

    #[test]
    fn arch_placeholder_is_replaced() {
        let suffix = asset_suffix(&package());
        assert!(suffix == "-windows-x64.msi" || suffix == "-windows-arm64.msi");
    }

    #[test]
    fn picks_the_matching_asset() {
        let release = Release {
            tag_name: "v1".into(),
            assets: vec![
                asset(
                    "app-1-windows-x64.msi.asc",
                    "https://github.com/a",
                    Some(GOOD_DIGEST),
                ),
                asset(
                    "app-1-windows-x64.msi",
                    "https://github.com/b",
                    Some(GOOD_DIGEST),
                ),
            ],
        };
        let picked = select_asset(&release, "-windows-x64.msi").unwrap();
        assert_eq!(picked.name, "app-1-windows-x64.msi");
    }

    #[test]
    fn refuses_unsafe_downloads() {
        let release = |a: Asset| Release {
            tag_name: "v1".into(),
            assets: vec![a],
        };
        assert!(
            select_asset(
                &release(asset(
                    "x.msi",
                    "https://evil.example/x.msi",
                    Some(GOOD_DIGEST)
                )),
                ".msi"
            )
            .is_err()
        );
        assert!(
            select_asset(
                &release(asset(
                    "..\\x.msi",
                    "https://github.com/x.msi",
                    Some(GOOD_DIGEST)
                )),
                ".msi"
            )
            .is_err()
        );
        assert!(
            select_asset(
                &release(asset("x.msi", "https://github.com/x", None)),
                ".exe"
            )
            .is_err()
        );
    }

    #[test]
    fn requires_a_sha256_digest() {
        assert!(expected_sha256(&asset("x", "https://github.com/x", Some(GOOD_DIGEST))).is_ok());
        assert!(expected_sha256(&asset("x", "https://github.com/x", None)).is_err());
        assert!(expected_sha256(&asset("x", "https://github.com/x", Some("sha1:abc"))).is_err());
        assert!(expected_sha256(&asset("x", "https://github.com/x", Some("sha256:zz"))).is_err());
    }

    #[test]
    fn msi_install_is_passive_without_reboot() {
        let cmd = installer_cmd(&package(), Path::new("C:\\Temp\\a b.msi"));
        assert_eq!(cmd.program, "msiexec");
        assert_eq!(
            cmd.args,
            ["/i", "C:\\Temp\\a b.msi", "/passive", "/norestart"]
        );
    }

    #[test]
    fn msi_exit_codes_are_interpreted() {
        assert_eq!(interpret_exit(Some(0)), InstallOutcome::Installed);
        assert_eq!(interpret_exit(Some(3010)), InstallOutcome::Installed);
        assert_eq!(interpret_exit(Some(1638)), InstallOutcome::AlreadyInstalled);
        assert_eq!(interpret_exit(Some(1603)), InstallOutcome::Failed);
    }

    /// Downloads the real installer and checks its hash, without installing.
    /// Run with `cargo test real_download -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_download_is_verified() {
        let download = download(&package(), |line| println!("{line}")).unwrap();
        assert!(download.path.exists());
    }
}
