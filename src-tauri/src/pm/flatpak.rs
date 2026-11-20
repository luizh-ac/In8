use super::InstallOutcome;
use crate::catalog::FlatpakPackage;
use crate::error::Result;
use crate::exec::{self, Cmd};

pub const FLATHUB_NAME: &str = "flathub";
pub const FLATHUB_REPO: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";

pub fn is_available() -> bool {
    exec::capture(&Cmd::new("flatpak").arg("--version")).is_some()
}

/// Name and `.flatpakrepo` URL of the remote the package comes from (Flathub by default).
pub fn remote_of(package: &FlatpakPackage) -> (&str, &str) {
    package
        .remote()
        .map_or((FLATHUB_NAME, FLATHUB_REPO), |remote| {
            (remote.name.as_str(), remote.url.as_str())
        })
}

/// Per-user installs need no root; this makes sure the remote exists for the user installation.
pub fn add_remote_cmd(name: &str, url: &str) -> Cmd {
    Cmd::new("flatpak").args(["remote-add", "--user", "--if-not-exists", name, url])
}

pub fn install_cmd(package: &FlatpakPackage) -> Cmd {
    let (remote, _) = remote_of(package);
    Cmd::new("flatpak").args([
        "install",
        "--user",
        "--noninteractive",
        remote,
        package.id(),
    ])
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
    use crate::catalog::FlatpakRemote;

    #[test]
    fn flathub_install_is_per_user_and_non_interactive() {
        let cmd = install_cmd(&FlatpakPackage::Id("com.spotify.Client".into()));
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
    fn custom_remote_is_used_for_install_and_setup() {
        let package = FlatpakPackage::WithRemote {
            id: "ai.storyteller.artcraft-launcher".into(),
            remote: FlatpakRemote {
                name: "artcraft-oci".into(),
                url: "https://getartcraft.com/artcraft-oci.flatpakrepo".into(),
            },
        };
        assert!(
            install_cmd(&package)
                .args
                .contains(&"artcraft-oci".to_string())
        );
        let (name, url) = remote_of(&package);
        let setup = add_remote_cmd(name, url);
        assert_eq!(
            setup.args,
            [
                "remote-add",
                "--user",
                "--if-not-exists",
                "artcraft-oci",
                "https://getartcraft.com/artcraft-oci.flatpakrepo"
            ]
        );
    }
}
