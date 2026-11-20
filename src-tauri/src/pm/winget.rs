use std::collections::HashSet;

use super::InstallOutcome;
use crate::catalog::{WingetPackage, WingetSource};
use crate::error::Result;
use crate::exec::{self, Cmd};

/// APPINSTALLER_CLI_ERROR_PACKAGE_ALREADY_INSTALLED
const ALREADY_INSTALLED: i32 = 0x8A15_0061_u32 as i32;
/// APPINSTALLER_CLI_ERROR_UPDATE_NOT_APPLICABLE: returned by `install` when the package is already up to date.
const NO_APPLICABLE_UPDATE: i32 = 0x8A15_002B_u32 as i32;

pub fn is_available() -> bool {
    exec::capture(&Cmd::new("winget").arg("--version")).is_some()
}

/// Registers App Installer (which brings winget) for this user. On a fresh Windows it's often there but
/// not registered yet. The script is constant; nothing from data goes into it.
const REPAIR_SCRIPT: &str = "$ProgressPreference = 'SilentlyContinue'; \
try { Add-AppxPackage -RegisterByFamilyName -MainPackage Microsoft.DesktopAppInstaller_8wekyb3d8bbwe -ErrorAction Stop; \
Write-Output 'winget is registered.' } \
catch { Write-Output $_.Exception.Message; \
Write-Output 'App Installer, which brings winget, is not on this system. Get it from the Microsoft Store or https://aka.ms/getwinget.'; \
exit 1 }";

pub fn repair_cmd() -> Cmd {
    Cmd::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", REPAIR_SCRIPT])
}

pub fn install_cmd(package: &WingetPackage) -> Cmd {
    Cmd::new("winget").args([
        "install",
        "--id",
        package.id(),
        "--exact",
        "--source",
        source_name(package.source()),
        "--silent",
        "--accept-package-agreements",
        "--accept-source-agreements",
        "--disable-interactivity",
    ])
}

pub fn interpret_exit(code: Option<i32>) -> InstallOutcome {
    match code {
        Some(0) => InstallOutcome::Installed,
        Some(ALREADY_INSTALLED | NO_APPLICABLE_UPDATE) => InstallOutcome::AlreadyInstalled,
        _ => InstallOutcome::Failed,
    }
}

/// Which of these packages are installed. The bulk listing is fast but misses some apps that
/// winget does match when asked by ID (e.g. Brave, Discord), so those are checked one by one.
pub fn installed_among(packages: &[&WingetPackage]) -> Result<Vec<String>> {
    let listed: HashSet<String> = list_all()?
        .into_iter()
        .map(|id| id.to_lowercase())
        .collect();
    let (mut found, missing): (Vec<&WingetPackage>, Vec<&WingetPackage>) = packages
        .iter()
        .partition(|package| listed.contains(&package.id().to_lowercase()));
    found.extend(installed_by_id(&missing));
    Ok(found
        .into_iter()
        .map(|package| package.id().to_string())
        .collect())
}

/// Checks packages in parallel with `winget list --id <id> --exact`.
fn installed_by_id<'a>(packages: &[&'a WingetPackage]) -> Vec<&'a WingetPackage> {
    const WORKERS: usize = 6;
    if packages.is_empty() {
        return Vec::new();
    }
    let chunk_size = packages.len().div_ceil(WORKERS);
    std::thread::scope(|scope| {
        let workers: Vec<_> = packages
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .copied()
                        .filter(|package| is_installed(package))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap_or_default())
            .collect()
    })
}

fn is_installed(package: &WingetPackage) -> bool {
    Cmd::new("winget")
        .args([
            "list",
            "--id",
            package.id(),
            "--exact",
            "--source",
            source_name(package.source()),
            "--accept-source-agreements",
            "--disable-interactivity",
        ])
        .to_command()
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Uses `winget list` rather than `winget export`: export skips packages whose installed
/// version is no longer in the source (e.g. an app with a pending update). Each source is
/// queried on its own because only then does winget match installed apps to that source's IDs.
fn list_all() -> Result<Vec<String>> {
    let mut ids = Vec::new();
    for source in [WingetSource::Winget, WingetSource::Msstore] {
        let output = Cmd::new("winget")
            .args([
                "list",
                "--source",
                source_name(source),
                "--accept-source-agreements",
                "--disable-interactivity",
            ])
            .to_command()
            .output()?;
        ids.extend(parse_list_table(&String::from_utf8_lossy(&output.stdout)));
    }
    Ok(ids)
}

fn source_name(source: WingetSource) -> &'static str {
    match source {
        WingetSource::Winget => "winget",
        WingetSource::Msstore => "msstore",
    }
}

/// Reads the Id column of `winget list`. Header labels follow the system language,
/// so only the column order (Name, Id, Version, …) is assumed, never the label text.
fn parse_list_table(output: &str) -> Vec<String> {
    // Spinner frames are drawn with `\r` before the header; keep only what was drawn last.
    let lines: Vec<&str> = output
        .lines()
        .map(|line| line.rsplit('\r').next().unwrap_or(line))
        .collect();
    let Some(separator) = lines.iter().position(|line| {
        let line = line.trim();
        line.len() >= 3 && line.chars().all(|c| c == '-')
    }) else {
        return Vec::new();
    };
    let Some(header) = separator.checked_sub(1).map(|i| lines[i]) else {
        return Vec::new();
    };
    let starts = column_starts(header);
    let (Some(&id_start), Some(&id_end)) = (starts.get(1), starts.get(2)) else {
        return Vec::new();
    };
    lines[separator + 1..]
        .iter()
        .filter_map(|line| {
            let id: String = line
                .chars()
                .skip(id_start)
                .take(id_end - id_start)
                .collect();
            let id = id.trim();
            (!id.is_empty()).then(|| id.to_string())
        })
        .collect()
}

/// Character offsets where header columns start: a word at the line start or after two or more spaces.
fn column_starts(header: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut spaces = 2;
    for (index, c) in header.chars().enumerate() {
        if c == ' ' {
            spaces += 1;
        } else {
            if spaces >= 2 {
                starts.push(index);
            }
            spaces = 0;
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_cmd_uses_exact_id_and_source() {
        let cmd = install_cmd(&WingetPackage::WithSource {
            id: "9WZDNCRFJ3TJ".into(),
            source: WingetSource::Msstore,
        });
        assert_eq!(cmd.program, "winget");
        assert_eq!(
            cmd.args,
            [
                "install",
                "--id",
                "9WZDNCRFJ3TJ",
                "--exact",
                "--source",
                "msstore",
                "--silent",
                "--accept-package-agreements",
                "--accept-source-agreements",
                "--disable-interactivity",
            ]
        );
    }

    #[test]
    fn plain_id_uses_winget_source() {
        let cmd = install_cmd(&WingetPackage::Id("Google.Chrome".into()));
        assert!(
            cmd.args
                .windows(2)
                .any(|pair| pair == ["--source", "winget"])
        );
    }

    #[test]
    fn exit_codes_are_interpreted() {
        assert_eq!(interpret_exit(Some(0)), InstallOutcome::Installed);
        assert_eq!(
            interpret_exit(Some(-1978335135)),
            InstallOutcome::AlreadyInstalled
        );
        assert_eq!(
            interpret_exit(Some(-1978335189)),
            InstallOutcome::AlreadyInstalled
        );
        assert_eq!(interpret_exit(Some(1)), InstallOutcome::Failed);
        assert_eq!(interpret_exit(None), InstallOutcome::Failed);
    }

    #[test]
    fn parses_list_table_after_spinner() {
        let output = "\r   - \r   \\ \rName            Id                     Version  Available Source\n\
                      ------------------------------------------------------------------\n\
                      Brave           Brave.Brave            158.1    159.0     winget\n\
                      WhatsApp Beta   9NBDXK71NK08           2.26              msstore\n\
                      Ação Legal      MSIX\\Some.App_1.0_x64  1.0\n";
        assert_eq!(
            parse_list_table(output),
            ["Brave.Brave", "9NBDXK71NK08", "MSIX\\Some.App_1.0_x64"]
        );
    }

    #[test]
    fn parses_localized_header_with_multi_word_labels() {
        let output = "Nome do pacote  ID             Versão  Disponível  Origem\n\
                      --------------------------------------------------------\n\
                      Discord         Discord.Discord 1.0    1.1         winget\n";
        assert_eq!(parse_list_table(output), ["Discord.Discord"]);
    }

    #[test]
    fn empty_or_unexpected_output_gives_no_ids() {
        assert!(parse_list_table("").is_empty());
        assert!(parse_list_table("No installed package found matching input criteria.").is_empty());
    }
}
