//! WSL (Windows only at runtime): status, running commands inside a distro, and the
//! environment setup actions (distro, systemd, Docker Engine, VS Code, `.wslconfig`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::exec::{self, Cmd};
use crate::ini;
use crate::platform::parse_os_release;
use crate::pm::apt;

pub const VSCODE_WSL_EXTENSION: &str = "ms-vscode-remote.remote-wsl";
const DOCKER_DESKTOP_DISTROS: &[&str] = &["docker-desktop", "docker-desktop-data"];
const WSLCONFIG_SECTION: &str = "wsl2";

/// Mirrored in `src/lib/types.ts` (`WslStatus`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WslStatus {
    pub installed: bool,
    pub version: Option<String>,
    /// User distros (Docker Desktop's internal distros are left out).
    pub distros: Vec<WslDistro>,
    pub docker_desktop: bool,
    /// The distro In8 works with: the requested one, else the default, else the first.
    pub active: Option<DistroDetails>,
    /// `None` when VS Code isn't installed.
    pub vscode_extension: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WslDistro {
    pub name: String,
    pub running: bool,
    pub version: u8,
    pub is_default: bool,
}

/// Mirrored in `src/lib/types.ts` (`DistroDetails`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistroDetails {
    pub name: String,
    pub os_name: String,
    pub user: String,
    /// False until the first launch created a Linux user (commands then run as root).
    pub has_user: bool,
    pub systemd: bool,
    pub apt: bool,
    pub docker: bool,
}

impl DistroDetails {
    /// Distros In8 can install apps into.
    pub fn is_ready(&self) -> bool {
        self.has_user && self.apt
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineDistro {
    pub name: String,
    pub friendly_name: String,
}

/// Environment actions requested from the WSL screen.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum WslAction {
    InstallWsl,
    InstallDistro { distro: String },
    EnableSystemd,
    DockerEngine,
    VscodeExtension,
}

/// `wsl.exe` with UTF-8 output (it prints UTF-16 by default).
pub fn wsl() -> Cmd {
    Cmd::new("wsl.exe").env("WSL_UTF8", "1")
}

/// Runs `cmd` inside a distro with `--exec`, so the distro's shell never parses anything.
pub fn inside(distro: &str, root: bool, cmd: Cmd) -> Cmd {
    let mut wrapped = wsl().args(["-d", distro]);
    if root {
        wrapped = wrapped.args(["-u", "root"]);
    }
    let Cmd {
        program,
        args,
        input,
        ..
    } = cmd;
    let mut wrapped = wrapped.arg("--exec").arg(program).args(args);
    wrapped.input = input;
    wrapped
}

/// Runs `cmd` in the distro as its default (Linux) user, starting in their home folder.
pub fn in_home(distro: &str, cmd: Cmd) -> Cmd {
    let Cmd {
        program,
        args,
        input,
        ..
    } = cmd;
    let mut wrapped = wsl()
        .args(["-d", distro, "--cd", "~", "--exec"])
        .arg(program)
        .args(args);
    wrapped.input = input;
    wrapped
}

/// VS Code's `code` script for use inside WSL (next to `code.cmd`), as a Linux path. Asks `wslpath`,
/// so it works whatever the distro's mount root is.
pub fn code_shim_in(distro: &str) -> Option<String> {
    let shim = find_code()?.with_file_name("code");
    if !shim.is_file() {
        return None;
    }
    let output = exec::capture(
        &wsl()
            .args(["-d", distro, "--exec", "wslpath", "-u"])
            .arg(shim.to_string_lossy()),
    )?;
    let path = output.trim();
    (!path.is_empty()).then(|| path.to_string())
}

pub fn status(preferred: Option<&str>) -> WslStatus {
    let version = exec::capture(&wsl().arg("--version")).map(|output| parse_version(&output));
    let installed = version.is_some();
    let all = if installed {
        exec::capture_stdout(&wsl().args(["--list", "--verbose"]))
            .map(|output| parse_list_verbose(&output))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let docker_desktop = all
        .iter()
        .any(|d| DOCKER_DESKTOP_DISTROS.contains(&d.name.as_str()));
    let distros: Vec<WslDistro> = all
        .into_iter()
        .filter(|d| !DOCKER_DESKTOP_DISTROS.contains(&d.name.as_str()))
        .collect();
    let active = preferred
        .and_then(|name| distros.iter().find(|d| d.name == name))
        .or_else(|| distros.iter().find(|d| d.is_default))
        .or_else(|| distros.first())
        .map(|d| details(&d.name));
    WslStatus {
        installed,
        version: version.flatten(),
        distros,
        docker_desktop,
        active,
        vscode_extension: vscode_extension_installed(),
    }
}

fn details(name: &str) -> DistroDetails {
    let run = |cmd: Cmd| exec::capture(&inside(name, false, cmd));
    let os_name = run(Cmd::new("cat").arg("/etc/os-release"))
        .map(|content| parse_os_release(&content).name)
        .unwrap_or_default();
    let user = run(Cmd::new("whoami"))
        .map(|user| user.trim().to_string())
        .unwrap_or_default();
    let systemd = run(Cmd::new("ps").args(["-p", "1", "-o", "comm="]))
        .is_some_and(|comm| comm.trim() == "systemd");
    let apt = run(Cmd::new("test").args(["-x", "/usr/bin/apt-get"])).is_some();
    let docker = apt
        && exec::capture_stdout(&inside(name, false, apt::query_cmd(&["docker.io"])))
            .is_some_and(|output| apt::parse_query(&output).iter().any(|p| p == "docker.io"));
    DistroDetails {
        name: name.to_string(),
        os_name,
        has_user: !user.is_empty() && user != "root",
        user,
        systemd,
        apt,
        docker,
    }
}

pub fn online_distros() -> Vec<OnlineDistro> {
    exec::capture_stdout(&wsl().args(["--list", "--online"]))
        .map(|output| parse_online(&output))
        .unwrap_or_default()
}

/// Opens the distro in its own console window, where its first launch asks for a user name and password.
pub fn open_terminal(distro: &str) -> Result<()> {
    if !is_distro_name(distro) {
        return Err(Error::Wsl(format!("invalid distro name: {distro}")));
    }
    let mut command = std::process::Command::new("wsl.exe");
    command.args(["-d", distro]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        command.creation_flags(CREATE_NEW_CONSOLE);
    }
    command.spawn()?;
    Ok(())
}

pub fn is_distro_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// `/etc/wsl.conf` with systemd turned on, keeping everything else.
pub fn wsl_conf_with_systemd(distro: &str) -> String {
    let current =
        exec::capture_stdout(&inside(distro, false, Cmd::new("cat").arg("/etc/wsl.conf")))
            .unwrap_or_default();
    ini::set(&current, "boot", "systemd", Some("true"))
}

/// `code.cmd` from a per-user or per-machine VS Code install. Looked up on disk because
/// VS Code may have been installed earlier in the same run, after In8 read its PATH.
pub fn find_code() -> Option<PathBuf> {
    let user = std::env::var_os("LOCALAPPDATA")
        .map(|dir| PathBuf::from(dir).join(r"Programs\Microsoft VS Code\bin\code.cmd"));
    let machine = std::env::var_os("ProgramFiles")
        .map(|dir| PathBuf::from(dir).join(r"Microsoft VS Code\bin\code.cmd"));
    [user, machine]
        .into_iter()
        .flatten()
        .find(|path| path.is_file())
}

fn vscode_extension_installed() -> Option<bool> {
    let code = find_code()?;
    let output = exec::capture(&Cmd::new(code.to_string_lossy()).arg("--list-extensions"))?;
    Some(
        output
            .lines()
            .any(|line| line.trim().eq_ignore_ascii_case(VSCODE_WSL_EXTENSION)),
    )
}

/// First line's value, e.g. "WSL version: 2.7.11.0" (labels follow the Windows language).
fn parse_version(output: &str) -> Option<String> {
    output
        .lines()
        .next()
        .and_then(|line| line.split_once(':'))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Rows look like `* Ubuntu   Running   2`. Only the shape is used, not the header labels.
fn parse_list_verbose(output: &str) -> Vec<WslDistro> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let is_default = line.starts_with('*');
            let tokens: Vec<&str> = line.trim_start_matches('*').split_whitespace().collect();
            let (name, rest) = tokens.split_first()?;
            let (version, state) = rest.split_last()?;
            let version: u8 = version.parse().ok()?;
            Some(WslDistro {
                name: (*name).to_string(),
                running: state.join(" ").eq_ignore_ascii_case("running"),
                version,
                is_default,
            })
        })
        .collect()
}

/// Rows after the `NAME  FRIENDLY NAME` header.
fn parse_online(output: &str) -> Vec<OnlineDistro> {
    output
        .lines()
        .skip_while(|line| line.split_whitespace().next() != Some("NAME"))
        .skip(1)
        .filter_map(|line| {
            let (name, friendly) = line.trim().split_once(char::is_whitespace)?;
            Some(OnlineDistro {
                name: name.to_string(),
                friendly_name: friendly.trim().to_string(),
            })
        })
        .collect()
}

// ---- .wslconfig ----------------------------------------------------------------------

/// The `[wsl2]` settings In8 edits. Mirrored in `src/lib/types.ts` (`WslConfig`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WslConfig {
    pub memory: Option<String>,
    pub processors: Option<u32>,
    pub swap: Option<String>,
}

fn wslconfig_path() -> Result<PathBuf> {
    std::env::var_os("USERPROFILE")
        .map(|home| PathBuf::from(home).join(".wslconfig"))
        .ok_or_else(|| Error::Wsl("USERPROFILE isn't set".into()))
}

fn read_wslconfig_text() -> Result<String> {
    match std::fs::read_to_string(wslconfig_path()?) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

pub fn read_wslconfig() -> Result<WslConfig> {
    let content = read_wslconfig_text()?;
    Ok(WslConfig {
        memory: ini::get(&content, WSLCONFIG_SECTION, "memory"),
        processors: ini::get(&content, WSLCONFIG_SECTION, "processors")
            .and_then(|p| p.parse().ok()),
        swap: ini::get(&content, WSLCONFIG_SECTION, "swap"),
    })
}

/// The file as it would be after saving `config`.
pub fn preview_wslconfig(config: &WslConfig) -> Result<String> {
    validate_wslconfig(config)?;
    let mut content = read_wslconfig_text()?;
    let processors = config.processors.map(|p| p.to_string());
    for (key, value) in [
        ("memory", config.memory.as_deref()),
        ("processors", processors.as_deref()),
        ("swap", config.swap.as_deref()),
    ] {
        content = ini::set(&content, WSLCONFIG_SECTION, key, value);
    }
    Ok(content)
}

/// Saves `.wslconfig`, keeping a copy of the original file the first time.
pub fn save_wslconfig(config: &WslConfig) -> Result<()> {
    let content = preview_wslconfig(config)?;
    let path = wslconfig_path()?;
    let backup = path.with_extension("wslconfig.in8-backup");
    if path.exists() && !backup.exists() {
        std::fs::copy(&path, &backup)?;
    }
    std::fs::write(&path, content)?;
    Ok(())
}

fn is_size(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    let digits = upper
        .strip_suffix("GB")
        .or_else(|| upper.strip_suffix("MB"))
        .unwrap_or("");
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

fn validate_wslconfig(config: &WslConfig) -> Result<()> {
    if let Some(memory) = &config.memory
        && !is_size(memory)
    {
        return Err(Error::Wsl(format!(
            "memory must look like 8GB or 4096MB, not {memory}"
        )));
    }
    if let Some(swap) = &config.swap
        && swap != "0"
        && !is_size(swap)
    {
        return Err(Error::Wsl(format!(
            "swap must look like 4GB, 512MB or 0, not {swap}"
        )));
    }
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
    if let Some(processors) = config.processors
        && !(1..=cpus).contains(&processors)
    {
        return Err(Error::Wsl(format!(
            "processors must be between 1 and {cpus}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_uses_exec_and_keeps_arguments_separate() {
        let cmd = inside(
            "Ubuntu",
            true,
            Cmd::new("tee").arg("/etc/wsl.conf").input("[boot]\n"),
        );
        assert_eq!(cmd.program, "wsl.exe");
        assert_eq!(
            cmd.args,
            [
                "-d",
                "Ubuntu",
                "-u",
                "root",
                "--exec",
                "tee",
                "/etc/wsl.conf"
            ]
        );
        assert_eq!(cmd.input.as_deref(), Some("[boot]\n"));
        assert!(cmd.env.contains(&("WSL_UTF8".into(), "1".into())));
    }

    #[test]
    fn parses_localized_version() {
        let output = "Versão do WSL: 2.7.11.0\nVersão do kernel: 6.18.33.2-2\n";
        assert_eq!(parse_version(output).as_deref(), Some("2.7.11.0"));
    }

    #[test]
    fn parses_distro_list() {
        let output = "  NAME              STATE           VERSION\n\
                      * Ubuntu            Running         2\n\
                      \x20 docker-desktop    Stopped         2\n";
        assert_eq!(
            parse_list_verbose(output),
            [
                WslDistro {
                    name: "Ubuntu".into(),
                    running: true,
                    version: 2,
                    is_default: true
                },
                WslDistro {
                    name: "docker-desktop".into(),
                    running: false,
                    version: 2,
                    is_default: false
                },
            ]
        );
        assert!(
            parse_list_verbose("Windows Subsystem for Linux has no installed distributions.")
                .is_empty()
        );
    }

    #[test]
    fn parses_online_distros_after_header() {
        let output = "A seguir está uma lista de distribuições válidas.\n\nNAME            FRIENDLY NAME\n\
                      Ubuntu-26.04    Ubuntu 26.04 LTS\nopenSUSE-Tumbleweed   openSUSE Tumbleweed\n";
        let distros = parse_online(output);
        assert_eq!(distros.len(), 2);
        assert_eq!(distros[0].name, "Ubuntu-26.04");
        assert_eq!(distros[0].friendly_name, "Ubuntu 26.04 LTS");
    }

    #[test]
    fn distro_names_are_validated() {
        assert!(is_distro_name("Ubuntu-26.04"));
        assert!(!is_distro_name("Ubuntu; rm -rf"));
        assert!(!is_distro_name(""));
    }

    #[test]
    fn wslconfig_values_are_validated() {
        let ok = WslConfig {
            memory: Some("8GB".into()),
            processors: Some(1),
            swap: Some("0".into()),
        };
        assert!(validate_wslconfig(&ok).is_ok());
        let bad_memory = WslConfig {
            memory: Some("lots".into()),
            ..WslConfig::default()
        };
        assert!(validate_wslconfig(&bad_memory).is_err());
        let bad_cpus = WslConfig {
            processors: Some(0),
            ..WslConfig::default()
        };
        assert!(validate_wslconfig(&bad_cpus).is_err());
    }

    /// Reads the real WSL setup. Run with `cargo test real_wsl -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_wsl_status() {
        let status = status(None);
        println!("{status:#?}");
        assert!(status.installed);
    }
}
