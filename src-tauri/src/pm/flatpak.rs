use super::InstallOutcome;
use crate::error::Result;
use crate::exec::{self, Cmd};

const FLATHUB_REPO: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";

pub fn is_available() -> bool {
    exec::capture(&Cmd::new("flatpak").arg("--version")).is_some()
}

/// Per-user installs need no root; this makes sure Flathub exists for the user installation.
pub fn add_flathub_cmd() -> Cmd {
    Cmd::new("flatpak").args([
        "remote-add",
        "--user",
        "--if-not-exists",
        "flathub",
        FLATHUB_REPO,
    ])
}

pub fn install_cmd(app_id: &str) -> Cmd {
    Cmd::new("flatpak").args(["install", "--user", "--noninteractive", "flathub", app_id])
}

pub fn interpret_exit(code: Option<i32>) -> InstallOutcome {
    if code == Some(0) {
        InstallOutcome::Installed
    } else {
        InstallOutcome::Failed
    }
}

pub fn list_installed() -> Result<Vec<String>> {
    let output = Cmd::new("flatpak")
        .args(["list", "--app", "--columns=application"])
        .to_command()
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_per_user_and_non_interactive() {
        let cmd = install_cmd("com.spotify.Client");
        assert_eq!(cmd.program, "flatpak");
        assert_eq!(
            cmd.args,
            [
                "install",
                "--user",
                "--noninteractive",
                "flathub",
                "com.spotify.Client"
            ]
        );
    }

    #[test]
    fn flathub_setup_is_idempotent() {
        assert!(
            add_flathub_cmd()
                .args
                .contains(&"--if-not-exists".to_string())
        );
    }
}
