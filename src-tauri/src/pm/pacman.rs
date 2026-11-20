//! pacman (Arch and derivatives). Installs run as root through the pkexec helper.

use crate::exec::Cmd;

/// Arch doesn't support partial upgrades, so installing syncs and upgrades the system too (`-Syu`).
pub fn install_cmd(packages: &[&str]) -> Cmd {
    Cmd::new("pacman")
        .args(["-Syu", "--needed", "--noconfirm"])
        .args(packages.iter().copied())
}

/// `pacman -Q` prints `name version` for installed packages and errors (on stderr) for the rest.
pub fn query_cmd(packages: &[&str]) -> Cmd {
    Cmd::new("pacman").arg("-Q").args(packages.iter().copied())
}

pub fn parse_query(output: &str, packages: &[&str]) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| packages.contains(name))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_syncs_and_skips_installed_packages() {
        assert_eq!(
            install_cmd(&["git"]).args,
            ["-Syu", "--needed", "--noconfirm", "git"]
        );
    }

    #[test]
    fn query_keeps_only_installed_names() {
        assert_eq!(parse_query("git 2.56.1-1\n", &["git", "rustup"]), ["git"]);
    }
}
