//! Post-install tasks: configuration after the apps are in (Git identity, SSH key, VS Code extensions,
//! toolchains, folders, dotfiles), on Windows and/or inside WSL.
//!
//! The catalog is data; each action type is implemented here. User input only ever replaces a whole
//! argument written as `{{input}}`, after validation, and `command` actions may only run allowlisted programs.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, Target};
use crate::error::{Error, Result};
use crate::exec::{self, Cmd};
use crate::platform::{Os, PlatformInfo};
use crate::pm::{InstallAction, apt};
use crate::runner::{Task, TaskKind};
use crate::wsl::{self, DistroDetails};

const EMBEDDED_POSTINSTALL: &str = include_str!("../../catalog/postinstall.json");

/// Programs a `command` action may run.
const ALLOWED_PROGRAMS: &[&str] = &["git", "rustup"];
const SSH_KEY_FILE: &str = "id_ed25519";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostInstallCatalog {
    pub version: u32,
    pub tasks: Vec<PostTask>,
}

/// Mirrored in `src/lib/types.ts` (`PostTask`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostTask {
    pub id: String,
    pub name: String,
    pub description: String,
    pub targets: Vec<Target>,
    /// Catalog app the task needs (installed or selected for the same target).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<String>,
    #[serde(default)]
    pub inputs: Vec<Input>,
    /// Things to pick from (e.g. VS Code extensions).
    #[serde(default)]
    pub options: Vec<PickOption>,
    pub actions: Vec<PostAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
    pub id: String,
    pub label: String,
    pub kind: InputKind,
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub placeholder: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InputKind {
    Text,
    Email,
    /// `https://…` or `git@host:path`.
    GitUrl,
    /// A folder inside the home folder, like `dev` or `code/work`.
    Folder,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PickOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PostAction {
    /// An allowlisted program, run as the user in their home folder.
    Command { program: String, args: Vec<String> },
    /// `apt-get install` as root (WSL only).
    Apt { packages: Vec<String> },
    /// `mkdir -p` inside the home folder.
    CreateDir { path: String },
    /// `ssh-keygen` in a terminal window, so the user types the passphrase there.
    SshKey { comment: String },
    /// `code --install-extension` for each picked option.
    VscodeExtensions,
}

/// One post-install task to run. Mirrored in `src/lib/types.ts` (`PostInstallRequest`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostInstallRequest {
    pub id: String,
    pub target: Target,
    /// Input values by input ID (missing ones use the input's default).
    #[serde(default)]
    pub values: BTreeMap<String, String>,
    /// Picked option IDs.
    #[serde(default)]
    pub options: Vec<String>,
}

/// `ssh-keygen` for a key that doesn't exist yet. Mirrored nowhere (runner only).
#[derive(Debug, Clone)]
pub struct SshKeyJob {
    /// `None`: Windows (or native Linux); `Some`: inside this WSL distro.
    pub distro: Option<String>,
    pub comment: String,
}

fn is_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_extension_id(value: &str) -> bool {
    value.split_once('.').is_some_and(|(publisher, name)| {
        [publisher, name].iter().all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
    })
}

fn is_package_name(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "+-.".contains(c))
}

/// `{{id}}` → `Some("id")`.
fn placeholder(arg: &str) -> Option<&str> {
    arg.strip_prefix("{{")?.strip_suffix("}}")
}

/// Checks a value typed by the user. Returns a message for the UI when it's not acceptable.
pub fn validate_value(kind: InputKind, value: &str) -> std::result::Result<(), &'static str> {
    if value.trim().is_empty() {
        return Err("is required");
    }
    if value.chars().any(char::is_control) || value.len() > 300 {
        return Err("has characters In8 doesn't accept");
    }
    match kind {
        InputKind::Text => {
            if value.len() > 100 {
                return Err("is too long");
            }
        }
        InputKind::Email => {
            let valid = value.split_once('@').is_some_and(|(user, domain)| {
                !user.is_empty() && domain.contains('.') && !domain.ends_with('.')
            });
            if !valid || value.chars().any(char::is_whitespace) || value.starts_with('-') {
                return Err("must be an email address");
            }
        }
        InputKind::GitUrl => {
            let valid = (value.starts_with("https://") || value.starts_with("git@"))
                && !value.chars().any(char::is_whitespace);
            if !valid {
                return Err("must start with https:// or git@");
            }
        }
        InputKind::Folder => {
            let valid = !value.starts_with('/')
                && !value.starts_with('-')
                && value.split('/').all(|part| {
                    !part.is_empty()
                        && part != "."
                        && part != ".."
                        && part
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
                });
            if !valid {
                return Err("must be a folder inside your home folder, like dev or code/work");
            }
        }
    }
    Ok(())
}

impl PostInstallCatalog {
    pub fn load_embedded() -> Result<Self> {
        Self::parse(EMBEDDED_POSTINSTALL)
    }

    pub fn parse(json: &str) -> Result<Self> {
        let catalog: PostInstallCatalog =
            serde_json::from_str(json).map_err(|e| Error::Catalog(format!("postinstall: {e}")))?;
        let mut seen = HashSet::new();
        for task in &catalog.tasks {
            task.validate().map_err(|reason| {
                Error::Catalog(format!("post-install \"{}\": {reason}", task.id))
            })?;
            if !seen.insert(task.id.as_str()) {
                return Err(Error::Catalog(format!(
                    "post-install \"{}\": duplicate id",
                    task.id
                )));
            }
        }
        Ok(catalog)
    }

    pub fn get(&self, id: &str) -> Option<&PostTask> {
        self.tasks.iter().find(|task| task.id == id)
    }
}

impl PostTask {
    fn validate(&self) -> std::result::Result<(), String> {
        if !is_id(&self.id) || self.name.trim().is_empty() {
            return Err("bad id or missing name".into());
        }
        if self.targets.is_empty() || self.actions.is_empty() {
            return Err("needs targets and actions".into());
        }
        let inputs: HashSet<&str> = self.inputs.iter().map(|i| i.id.as_str()).collect();
        if inputs.len() != self.inputs.len() || !self.inputs.iter().all(|i| is_id(&i.id)) {
            return Err("input ids must be unique slugs".into());
        }
        for input in &self.inputs {
            if !input.default.is_empty() && validate_value(input.kind, &input.default).is_err() {
                return Err(format!("default of \"{}\" isn't valid", input.id));
            }
        }
        let check_value = |value: &str| match placeholder(value) {
            Some(id) if inputs.contains(id) => Ok(()),
            Some(id) => Err(format!("unknown input {{{{{id}}}}}")),
            None if value.contains("{{") => {
                Err("placeholders must be a whole argument".to_string())
            }
            None => Ok(()),
        };
        for action in &self.actions {
            match action {
                PostAction::Command { program, args } => {
                    if !ALLOWED_PROGRAMS.contains(&program.as_str()) {
                        return Err(format!("program \"{program}\" isn't allowed"));
                    }
                    for arg in args {
                        check_value(arg)?;
                    }
                }
                PostAction::Apt { packages } => {
                    if self.targets != [Target::Wsl] {
                        return Err("apt actions are only for WSL-only tasks".into());
                    }
                    if packages.is_empty() || !packages.iter().all(|p| is_package_name(p)) {
                        return Err("bad apt package name".into());
                    }
                }
                PostAction::CreateDir { path } => check_value(path)?,
                PostAction::SshKey { comment } => check_value(comment)?,
                PostAction::VscodeExtensions => {
                    if self.options.is_empty()
                        || !self.options.iter().all(|o| is_extension_id(&o.id))
                    {
                        return Err(
                            "VS Code extensions need options with publisher.name ids".into()
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Input values with defaults filled in, all validated.
    fn values(&self, request: &PostInstallRequest) -> Result<BTreeMap<String, String>> {
        let mut values = BTreeMap::new();
        for input in &self.inputs {
            let value = request
                .values
                .get(&input.id)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| input.default.clone());
            validate_value(input.kind, &value).map_err(|reason| {
                Error::PostInstall(format!("{}: {} {reason}", self.name, input.label))
            })?;
            values.insert(input.id.clone(), value);
        }
        Ok(values)
    }
}

fn fill(value: &str, values: &BTreeMap<String, String>) -> String {
    placeholder(value)
        .and_then(|id| values.get(id))
        .cloned()
        .unwrap_or_else(|| value.to_string())
}

/// Turns post-install requests into runner tasks. WSL tasks need a ready distro; `apt_update_planned`
/// says whether the queue already refreshes the distro's package lists.
pub fn plan(
    catalog: &PostInstallCatalog,
    platform: &PlatformInfo,
    distro: Option<&DistroDetails>,
    requests: &[PostInstallRequest],
    apt_update_planned: bool,
) -> Result<Vec<Task>> {
    let mut tasks: Vec<Task> = Vec::new();
    let mut seen = HashSet::new();
    let mut apt_updated = apt_update_planned;
    let setup = |id: String, label: String, action: InstallAction| Task {
        id,
        label,
        action,
        kind: TaskKind::Setup,
        needs: None,
    };

    for request in requests {
        if !seen.insert((request.id.as_str(), request.target)) {
            continue;
        }
        let task = catalog
            .get(&request.id)
            .ok_or_else(|| Error::UnknownItem(request.id.clone()))?;
        if !task.targets.contains(&request.target) {
            return Err(Error::Unavailable(format!(
                "{} ({:?})",
                task.name, request.target
            )));
        }
        let wsl_distro = match request.target {
            Target::Host => None,
            Target::Wsl => {
                if platform.os != Os::Windows {
                    return Err(Error::Wsl("WSL is only available on Windows".into()));
                }
                let distro = distro.filter(|d| d.is_ready()).ok_or_else(|| {
                    Error::Wsl("set up an Ubuntu or Debian distro with a Linux user first".into())
                })?;
                Some(distro.name.as_str())
            }
        };
        let values = task.values(request)?;
        let where_ = if wsl_distro.is_some() { " (WSL)" } else { "" };
        let base_id = format!(
            "post-{}@{}",
            task.id,
            if wsl_distro.is_some() { "wsl" } else { "host" }
        );
        let home = exec::home_dir();

        for (index, action) in task.actions.iter().enumerate() {
            let id = format!("{base_id}-{index}");
            let label = format!("{}{where_}", task.name);
            match action {
                PostAction::Command { program, args } => {
                    let args: Vec<String> = args.iter().map(|arg| fill(arg, &values)).collect();
                    let action = match wsl_distro {
                        // Environment variables don't cross into WSL, so `env` sets them there.
                        Some(name) => InstallAction::Command(wsl::in_home(
                            name,
                            Cmd::new("env")
                                .args(["GIT_TERMINAL_PROMPT=0", program.as_str()])
                                .args(args),
                        )),
                        None => {
                            let mut cmd = Cmd::new(program.as_str())
                                .args(args)
                                .env("GIT_TERMINAL_PROMPT", "0");
                            if let Some(home) = &home {
                                cmd = cmd.cwd(home);
                            }
                            InstallAction::HostTool(cmd)
                        }
                    };
                    tasks.push(setup(id, label, action));
                }
                PostAction::Apt { packages } => {
                    let Some(name) = wsl_distro else {
                        return Err(Error::Unavailable(task.name.clone()));
                    };
                    if !apt_updated {
                        tasks.push(setup(
                            "wsl-apt-update".into(),
                            format!("Update package lists in {name}"),
                            InstallAction::Command(wsl::inside(name, true, apt::update_cmd())),
                        ));
                        apt_updated = true;
                    }
                    let packages: Vec<&str> = packages.iter().map(String::as_str).collect();
                    tasks.push(setup(
                        id,
                        label,
                        InstallAction::Command(wsl::inside(
                            name,
                            true,
                            apt::install_cmd(&packages),
                        )),
                    ));
                }
                PostAction::CreateDir { path } => {
                    let path = fill(path, &values);
                    let action = match wsl_distro {
                        Some(name) => InstallAction::Command(wsl::in_home(
                            name,
                            Cmd::new("mkdir").args(["-p", "--", path.as_str()]),
                        )),
                        None => {
                            let home = home.clone().ok_or_else(|| {
                                Error::PostInstall("couldn't find your home folder".into())
                            })?;
                            InstallAction::CreateDir(home.join(&path))
                        }
                    };
                    tasks.push(setup(id, label, action));
                }
                PostAction::SshKey { comment } => {
                    tasks.push(setup(
                        id,
                        label,
                        InstallAction::SshKey(SshKeyJob {
                            distro: wsl_distro.map(str::to_string),
                            comment: fill(comment, &values),
                        }),
                    ));
                }
                PostAction::VscodeExtensions => {
                    let picked: Vec<&PickOption> = task
                        .options
                        .iter()
                        .filter(|option| request.options.contains(&option.id))
                        .collect();
                    if picked.is_empty() {
                        return Err(Error::PostInstall(format!(
                            "{}: pick at least one",
                            task.name
                        )));
                    }
                    if wsl_distro.is_some() {
                        // `code` inside WSL works through the WSL extension on the Windows side.
                        tasks.push(Task {
                            id: "vscode-wsl-extension".into(),
                            label: "VS Code WSL extension".into(),
                            action: InstallAction::VscodeExtension(
                                wsl::VSCODE_WSL_EXTENSION.into(),
                            ),
                            kind: TaskKind::Setup,
                            needs: None,
                        });
                    }
                    for option in picked {
                        let action = match wsl_distro {
                            Some(name) => InstallAction::VscodeWslExtension {
                                distro: name.to_string(),
                                extension: option.id.clone(),
                            },
                            None => InstallAction::VscodeExtension(option.id.clone()),
                        };
                        tasks.push(setup(
                            format!("{base_id}-{}", option.id),
                            format!("VS Code: {}{where_}", option.label),
                            action,
                        ));
                    }
                }
            }
        }
    }
    Ok(tasks)
}

/// What's already set up, so the screen can prefill and show state. Mirrored in `src/lib/types.ts`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostInstallState {
    pub host: SideState,
    /// `None` outside Windows or without a ready distro.
    pub wsl: Option<SideState>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SideState {
    pub git_name: Option<String>,
    pub git_email: Option<String>,
    /// Contents of `~/.ssh/id_ed25519.pub`, to copy into GitHub.
    pub ssh_public_key: Option<String>,
    /// Installed VS Code extension IDs, lowercase.
    pub extensions: Vec<String>,
}

fn trimmed(output: Option<String>) -> Option<String> {
    output
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Reads Git identity, SSH key and VS Code extensions on Windows and in the distro. Takes a few seconds.
pub fn read_state(platform: &PlatformInfo, distro: Option<&DistroDetails>) -> PostInstallState {
    let git = exec::locate("git");
    let git_get = |key: &str| {
        trimmed(exec::capture(
            &Cmd::new(git.as_str()).args(["config", "--global", key]),
        ))
    };
    let host_key = exec::home_dir()
        .map(|home| home.join(".ssh").join(format!("{SSH_KEY_FILE}.pub")))
        .and_then(|path| std::fs::read_to_string(path).ok());
    let host_extensions = wsl::find_code()
        .and_then(|code| exec::capture(&Cmd::new(code.to_string_lossy()).arg("--list-extensions")))
        .map(|output| {
            output
                .lines()
                .map(|line| line.trim().to_lowercase())
                .filter(|line| !line.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let host = SideState {
        git_name: git_get("user.name"),
        git_email: git_get("user.email"),
        ssh_public_key: trimmed(host_key),
        extensions: host_extensions,
    };

    let wsl = (platform.os == Os::Windows)
        .then_some(distro)
        .flatten()
        .filter(|d| d.is_ready())
        .map(|distro| {
            let run = |cmd: Cmd| trimmed(exec::capture(&wsl::in_home(&distro.name, cmd)));
            let extensions = run(Cmd::new("ls").arg(".vscode-server/extensions"))
                .map(|listing| parse_extension_folders(&listing))
                .unwrap_or_default();
            SideState {
                git_name: run(Cmd::new("git").args(["config", "--global", "user.name"])),
                git_email: run(Cmd::new("git").args(["config", "--global", "user.email"])),
                ssh_public_key: run(Cmd::new("cat").arg(format!(".ssh/{SSH_KEY_FILE}.pub"))),
                extensions,
            }
        });
    PostInstallState { host, wsl }
}

/// `~/.vscode-server/extensions` holds folders like `rust-lang.rust-analyzer-0.3.2-linux-x64`.
fn parse_extension_folders(listing: &str) -> Vec<String> {
    listing
        .split_whitespace()
        .filter_map(|folder| {
            let folder = folder.to_lowercase();
            // The version starts at the first "-<digit>".
            let cut = folder
                .char_indices()
                .find(|(i, c)| {
                    *c == '-' && folder[i + 1..].starts_with(|d: char| d.is_ascii_digit())
                })
                .map(|(i, _)| i)?;
            let id = &folder[..cut];
            is_extension_id(id).then(|| id.to_string())
        })
        .collect()
}

/// Runs an SSH key job: keeps an existing key; otherwise creates `~/.ssh` and opens `ssh-keygen` in a
/// terminal window, waiting for it to close. Returns the exit code and log lines.
pub fn run_ssh_key(job: &SshKeyJob, log: &mut dyn FnMut(String)) -> Option<i32> {
    match &job.distro {
        None => {
            let Some(home) = exec::home_dir() else {
                log("Couldn't find your home folder.".into());
                return None;
            };
            let dir = home.join(".ssh");
            let key = dir.join(SSH_KEY_FILE);
            if key.exists() {
                log(format!("{} already exists; kept it.", key.display()));
                return Some(0);
            }
            if let Err(error) = std::fs::create_dir_all(&dir) {
                log(format!("Couldn't create {}: {error}", dir.display()));
                return None;
            }
            log("Type a passphrase in the terminal window that opened.".into());
            let keygen = Cmd::new("ssh-keygen").args([
                "-t",
                "ed25519",
                "-C",
                job.comment.as_str(),
                "-f",
                &key.to_string_lossy(),
            ]);
            let code = wait_console(&keygen, log)?;
            log_public_key(std::fs::read_to_string(key.with_extension("pub")).ok(), log);
            Some(code)
        }
        Some(distro) => {
            let exists = exec::capture(&wsl::in_home(
                distro,
                Cmd::new("test").args(["-f", &format!(".ssh/{SSH_KEY_FILE}")]),
            ))
            .is_some();
            if exists {
                log(format!(
                    "~/.ssh/{SSH_KEY_FILE} already exists in {distro}; kept it."
                ));
                return Some(0);
            }
            if exec::capture(&wsl::in_home(
                distro,
                Cmd::new("mkdir").args(["-p", "-m", "700", ".ssh"]),
            ))
            .is_none()
            {
                log(format!("Couldn't create ~/.ssh in {distro}."));
                return None;
            }
            log("Type a passphrase in the terminal window that opened.".into());
            let keygen = wsl::in_home(
                distro,
                Cmd::new("ssh-keygen").args([
                    "-t",
                    "ed25519",
                    "-C",
                    job.comment.as_str(),
                    "-f",
                    &format!(".ssh/{SSH_KEY_FILE}"),
                ]),
            );
            let code = wait_console(&keygen, log)?;
            log_public_key(
                exec::capture(&wsl::in_home(
                    distro,
                    Cmd::new("cat").arg(format!(".ssh/{SSH_KEY_FILE}.pub")),
                )),
                log,
            );
            Some(code)
        }
    }
}

fn wait_console(cmd: &Cmd, log: &mut dyn FnMut(String)) -> Option<i32> {
    match cmd.to_console_command().status() {
        Ok(status) => status.code(),
        Err(error) => {
            log(format!("Couldn't start {}: {error}", cmd.program));
            None
        }
    }
}

fn log_public_key(key: Option<String>, log: &mut dyn FnMut(String)) {
    if let Some(key) = key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
        log(format!(
            "Public key (add it to GitHub → Settings → SSH keys): {key}"
        ));
    }
}

/// How an SSH key job shows up in dry runs and the run screen.
pub fn describe_ssh(job: &SshKeyJob) -> String {
    let place = job
        .distro
        .as_deref()
        .map_or_else(String::new, |distro| format!(" in {distro}"));
    format!(
        "ssh-keygen -t ed25519 -C {} -f ~/.ssh/{SSH_KEY_FILE}{place} (in a terminal window; skipped if the key exists)",
        job.comment
    )
}

/// Checks profile/backup post-install entries against the catalogs (built-in profiles only).
pub fn check_requests(
    catalog: &PostInstallCatalog,
    apps: &Catalog,
    requests: &[PostInstallRequest],
) -> std::result::Result<(), String> {
    for request in requests {
        let task = catalog
            .get(&request.id)
            .ok_or_else(|| format!("unknown post-install task {}", request.id))?;
        if !task.targets.contains(&request.target) {
            return Err(format!("{} can't target {:?}", request.id, request.target));
        }
        if let Some(app) = &task.requires
            && apps.get(app).is_none()
        {
            return Err(format!("{} requires unknown app {app}", request.id));
        }
        if let Some(bad) = request
            .options
            .iter()
            .find(|option| !task.options.iter().any(|o| &o.id == *option))
        {
            return Err(format!("{}: unknown option {bad}", request.id));
        }
    }
    Ok(())
}

/// Keeps the shape of untrusted post-install entries sane (profiles files, backups).
pub fn sanitize_requests(requests: &[PostInstallRequest]) -> Vec<PostInstallRequest> {
    let mut seen = HashSet::new();
    requests
        .iter()
        .filter(|r| is_id(&r.id) && seen.insert((r.id.clone(), r.target)))
        .take(50)
        .map(|r| PostInstallRequest {
            id: r.id.clone(),
            target: r.target,
            values: r
                .values
                .iter()
                .filter(|(key, value)| is_id(key) && value.len() <= 300)
                .take(20)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            options: r
                .options
                .iter()
                .filter(|o| is_extension_id(o))
                .take(100)
                .cloned()
                .collect(),
        })
        .collect()
}

/// Where a host folder task writes, for display.
pub fn describe_dir(path: &std::path::Path) -> String {
    format!("mkdir {}", PathBuf::from(path).display())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows() -> PlatformInfo {
        PlatformInfo {
            os: Os::Windows,
            arch: "x86_64",
            version: "test",
            distro: None,
            managers: Vec::new(),
            prefer_native: false,
        }
    }

    fn ubuntu() -> DistroDetails {
        DistroDetails {
            name: "Ubuntu".into(),
            os_name: "Ubuntu".into(),
            user: "me".into(),
            has_user: true,
            systemd: true,
            apt: true,
            docker: false,
        }
    }

    fn request(
        id: &str,
        target: Target,
        values: &[(&str, &str)],
        options: &[&str],
    ) -> PostInstallRequest {
        PostInstallRequest {
            id: id.into(),
            target,
            values: values
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            options: options.iter().map(|o| o.to_string()).collect(),
        }
    }

    fn ids(tasks: &[Task]) -> Vec<&str> {
        tasks.iter().map(|t| t.id.as_str()).collect()
    }

    #[test]
    fn embedded_postinstall_is_valid() {
        let catalog =
            PostInstallCatalog::load_embedded().expect("embedded post-install should be valid");
        assert!(catalog.get("git-identity").is_some());
    }

    #[test]
    fn validates_values() {
        assert!(validate_value(InputKind::Email, "me@example.com").is_ok());
        assert!(validate_value(InputKind::Email, "me@local").is_err());
        assert!(validate_value(InputKind::Email, "-o@x.com").is_err());
        assert!(validate_value(InputKind::GitUrl, "https://github.com/me/dotfiles.git").is_ok());
        assert!(validate_value(InputKind::GitUrl, "git@github.com:me/dotfiles.git").is_ok());
        assert!(validate_value(InputKind::GitUrl, "--upload-pack=x").is_err());
        assert!(validate_value(InputKind::GitUrl, "file:///etc").is_err());
        assert!(validate_value(InputKind::Folder, "code/work").is_ok());
        assert!(validate_value(InputKind::Folder, "../outside").is_err());
        assert!(validate_value(InputKind::Folder, "/etc").is_err());
        assert!(validate_value(InputKind::Folder, "a b").is_err());
        assert!(validate_value(InputKind::Text, "Ana\nrm").is_err());
        assert!(validate_value(InputKind::Text, "  ").is_err());
    }

    #[test]
    fn rejects_programs_outside_the_allowlist_and_partial_placeholders() {
        let task = |action: &str| {
            format!(
                r#"{{ "version": 1, "tasks": [ {{ "id": "x", "name": "X", "description": "", "targets": ["host"],
                    "inputs": [ {{ "id": "v", "label": "V", "kind": "text" }} ], "actions": [ {action} ] }} ] }}"#
            )
        };
        assert!(
            PostInstallCatalog::parse(&task(
                r#"{ "type": "command", "program": "git", "args": ["{{v}}"] }"#
            ))
            .is_ok()
        );
        assert!(
            PostInstallCatalog::parse(&task(
                r#"{ "type": "command", "program": "powershell", "args": [] }"#
            ))
            .is_err()
        );
        assert!(
            PostInstallCatalog::parse(&task(
                r#"{ "type": "command", "program": "git", "args": ["x={{v}}"] }"#
            ))
            .is_err()
        );
        assert!(
            PostInstallCatalog::parse(&task(
                r#"{ "type": "command", "program": "git", "args": ["{{nope}}"] }"#
            ))
            .is_err()
        );
        assert!(
            PostInstallCatalog::parse(&task(r#"{ "type": "apt", "packages": ["curl"] }"#)).is_err()
        );
    }

    #[test]
    fn plans_git_identity_with_values_as_whole_arguments() {
        let catalog = PostInstallCatalog::load_embedded().unwrap();
        let values = [("name", "Ana; rm -rf ~"), ("email", "ana@example.com")];
        let tasks = plan(
            &catalog,
            &windows(),
            Some(&ubuntu()),
            &[request("git-identity", Target::Wsl, &values, &[])],
            false,
        )
        .unwrap();
        assert_eq!(
            ids(&tasks),
            [
                "post-git-identity@wsl-0",
                "post-git-identity@wsl-1",
                "post-git-identity@wsl-2"
            ]
        );
        let InstallAction::Command(cmd) = &tasks[0].action else {
            panic!("expected a command")
        };
        assert_eq!(
            cmd.args,
            [
                "-d",
                "Ubuntu",
                "--cd",
                "~",
                "--exec",
                "env",
                "GIT_TERMINAL_PROMPT=0",
                "git",
                "config",
                "--global",
                "user.name",
                "Ana; rm -rf ~"
            ]
        );
        let missing = plan(
            &catalog,
            &windows(),
            Some(&ubuntu()),
            &[request(
                "git-identity",
                Target::Host,
                &[("name", "Ana")],
                &[],
            )],
            false,
        );
        assert!(matches!(missing, Err(Error::PostInstall(_))));
    }

    #[test]
    fn plans_apt_update_once_and_extensions_through_the_wsl_extension() {
        let catalog = PostInstallCatalog::load_embedded().unwrap();
        let requests = [
            request("rust-toolchain", Target::Wsl, &[], &[]),
            request("python-tools", Target::Wsl, &[], &[]),
            request(
                "vscode-extensions",
                Target::Wsl,
                &[],
                &["rust-lang.rust-analyzer", "not.listed"],
            ),
        ];
        let tasks = plan(&catalog, &windows(), Some(&ubuntu()), &requests, false).unwrap();
        assert_eq!(
            ids(&tasks),
            [
                "wsl-apt-update",
                "post-rust-toolchain@wsl-0",
                "post-rust-toolchain@wsl-1",
                "post-python-tools@wsl-0",
                "vscode-wsl-extension",
                "post-vscode-extensions@wsl-rust-lang.rust-analyzer",
            ]
        );
        let none_picked = [request("vscode-extensions", Target::Host, &[], &[])];
        assert!(plan(&catalog, &windows(), None, &none_picked, false).is_err());
        let no_distro = [request("python-tools", Target::Wsl, &[], &[])];
        assert!(matches!(
            plan(&catalog, &windows(), None, &no_distro, false),
            Err(Error::Wsl(_))
        ));
    }

    #[test]
    fn host_folder_stays_inside_home() {
        let catalog = PostInstallCatalog::load_embedded().unwrap();
        let tasks = plan(
            &catalog,
            &windows(),
            None,
            &[request("dev-folder", Target::Host, &[], &[])],
            false,
        )
        .unwrap();
        let InstallAction::CreateDir(path) = &tasks[0].action else {
            panic!("expected a folder")
        };
        assert!(path.ends_with("dev"));
        assert!(path.starts_with(exec::home_dir().unwrap()));
    }

    /// Reads this machine and its WSL distro (nothing changes).
    /// Run with `cargo test real_postinstall_state -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_postinstall_state() {
        let platform = PlatformInfo::detect();
        let distro = wsl::status(None).active;
        println!("{:#?}", read_state(&platform, distro.as_ref()));
        if let Some(distro) = distro {
            println!("code shim in WSL: {:?}", wsl::code_shim_in(&distro.name));
        }
    }

    #[test]
    fn parses_vscode_server_extension_folders() {
        let listing = "rust-lang.rust-analyzer-0.3.2-linux-x64\nms-python.python-2025.1.0\nextensions.json\n.obsolete";
        assert_eq!(
            parse_extension_folders(listing),
            ["rust-lang.rust-analyzer", "ms-python.python"]
        );
    }
}
