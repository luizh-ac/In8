//! apt (Debian, Ubuntu). For now used inside WSL distros, where commands run as root
//! through `wsl.exe -u root`; native Linux support (via pkexec) comes later.

use super::InstallOutcome;
use crate::exec::Cmd;

pub fn update_cmd() -> Cmd {
    Cmd::new("apt-get").arg("update")
}

/// `env` sets the variable inside Linux; no shell is involved.
pub fn install_cmd(packages: &[&str]) -> Cmd {
    Cmd::new("env")
        .args(["DEBIAN_FRONTEND=noninteractive", "apt-get", "install", "-y"])
        .args(packages.iter().copied())
}

pub fn interpret_exit(code: Option<i32>) -> InstallOutcome {
    if code == Some(0) {
        InstallOutcome::Installed
    } else {
        InstallOutcome::Failed
    }
}

/// Lists which of these packages are installed. dpkg-query exits with 1 when some
/// aren't installed but still prints the ones it found, so read stdout regardless.
pub fn query_cmd<S: AsRef<str>>(packages: &[S]) -> Cmd {
    Cmd::new("dpkg-query")
        .args(["-W", r"-f=${Package}\t${db:Status-Abbrev}\n"])
        .args(packages.iter().map(|p| p.as_ref().to_string()))
}

pub fn parse_query(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let (name, status) = line.split_once('\t')?;
            status.starts_with("ii").then(|| name.trim().to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_non_interactive() {
        let cmd = install_cmd(&["nodejs", "git"]);
        assert_eq!(cmd.program, "env");
        assert_eq!(
            cmd.args,
            [
                "DEBIAN_FRONTEND=noninteractive",
                "apt-get",
                "install",
                "-y",
                "nodejs",
                "git"
            ]
        );
    }

    #[test]
    fn query_lists_only_fully_installed_packages() {
        let output = "git\tii \npython3\tii \nold-thing\trc \n";
        assert_eq!(parse_query(output), ["git", "python3"]);
        assert!(parse_query("").is_empty());
    }
}
