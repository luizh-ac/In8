//! dnf (Fedora and other RPM distros). Installs run as root through the pkexec helper.

use crate::exec::Cmd;

pub fn install_cmd(packages: &[&str]) -> Cmd {
    Cmd::new("dnf")
        .args(["install", "-y"])
        .args(packages.iter().copied())
}

/// `rpm -q` prints the name of each installed package and a sentence for the others
/// (and exits non-zero), so only lines that are exactly a requested name count.
pub fn query_cmd(packages: &[&str]) -> Cmd {
    Cmd::new("rpm")
        .args(["-q", "--queryformat", "%{NAME}\\n"])
        .args(packages.iter().copied())
}

pub fn parse_query(output: &str, packages: &[&str]) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| packages.contains(line))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_non_interactive() {
        assert_eq!(install_cmd(&["git"]).args, ["install", "-y", "git"]);
    }

    #[test]
    fn query_keeps_only_installed_names() {
        let output = "git\npackage nodejs is not installed\n";
        assert_eq!(parse_query(output, &["git", "nodejs"]), ["git"]);
    }
}
