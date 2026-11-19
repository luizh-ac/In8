//! Minimal INI reading and editing for `/etc/wsl.conf` and `.wslconfig`.
//! Edits keep comments, blank lines and unrelated keys untouched.

fn header(line: &str) -> Option<&str> {
    line.strip_prefix('[')?.strip_suffix(']').map(str::trim)
}

fn is_comment(line: &str) -> bool {
    line.starts_with('#') || line.starts_with(';')
}

fn key_of(line: &str) -> Option<&str> {
    if is_comment(line) {
        return None;
    }
    line.split_once('=').map(|(key, _)| key.trim())
}

pub fn get(content: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    for line in content.lines().map(str::trim) {
        if let Some(name) = header(line) {
            in_section = name.eq_ignore_ascii_case(section);
        } else if in_section && key_of(line).is_some_and(|k| k.eq_ignore_ascii_case(key)) {
            return line
                .split_once('=')
                .map(|(_, value)| value.trim().to_string());
        }
    }
    None
}

/// Sets `section.key` to `value`, or removes it when `value` is `None`.
pub fn set(content: &str, section: &str, key: &str, value: Option<&str>) -> String {
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let mut in_section = false;
    // Index right after the last non-blank line of the section, where a new key goes.
    let mut section_end = None;
    let mut existing = None;

    for (index, line) in lines.iter().enumerate() {
        let line = line.trim();
        if let Some(name) = header(line) {
            in_section = name.eq_ignore_ascii_case(section);
            if in_section {
                section_end = Some(index + 1);
            }
            continue;
        }
        if !in_section {
            continue;
        }
        if !line.is_empty() {
            section_end = Some(index + 1);
        }
        if existing.is_none() && key_of(line).is_some_and(|k| k.eq_ignore_ascii_case(key)) {
            existing = Some(index);
        }
    }

    let entry = value.map(|value| format!("{key}={value}"));
    match (existing, entry) {
        (Some(index), Some(entry)) => lines[index] = entry,
        (Some(index), None) => {
            lines.remove(index);
        }
        (None, Some(entry)) => match section_end {
            Some(end) => lines.insert(end, entry),
            None => {
                if lines.last().is_some_and(|line| !line.trim().is_empty()) {
                    lines.push(String::new());
                }
                lines.push(format!("[{section}]"));
                lines.push(entry);
            }
        },
        (None, None) => {}
    }

    let mut output = lines.join("\n");
    output.push('\n');
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    const WSL_CONF: &str = "# managed by me\n[boot]\nsystemd=false\n\n[user]\ndefault=luizh\n";

    #[test]
    fn reads_values_case_insensitively() {
        assert_eq!(get(WSL_CONF, "BOOT", "SystemD").as_deref(), Some("false"));
        assert_eq!(get(WSL_CONF, "user", "default").as_deref(), Some("luizh"));
        assert_eq!(get(WSL_CONF, "boot", "command"), None);
    }

    #[test]
    fn replaces_existing_key_in_place() {
        let updated = set(WSL_CONF, "boot", "systemd", Some("true"));
        assert_eq!(
            updated,
            "# managed by me\n[boot]\nsystemd=true\n\n[user]\ndefault=luizh\n"
        );
    }

    #[test]
    fn adds_key_at_end_of_existing_section() {
        let updated = set(WSL_CONF, "boot", "command", Some("echo hi"));
        assert_eq!(
            updated,
            "# managed by me\n[boot]\nsystemd=false\ncommand=echo hi\n\n[user]\ndefault=luizh\n"
        );
    }

    #[test]
    fn creates_missing_section_and_file() {
        assert_eq!(
            set("", "boot", "systemd", Some("true")),
            "[boot]\nsystemd=true\n"
        );
        assert_eq!(
            set("[user]\ndefault=a", "wsl2", "memory", Some("8GB")),
            "[user]\ndefault=a\n\n[wsl2]\nmemory=8GB\n"
        );
    }

    #[test]
    fn removes_key_and_ignores_commented_keys() {
        let content = "[wsl2]\n# memory=4GB\nmemory=8GB\nswap=0\n";
        assert_eq!(
            set(content, "wsl2", "memory", None),
            "[wsl2]\n# memory=4GB\nswap=0\n"
        );
        assert_eq!(get(content, "wsl2", "memory").as_deref(), Some("8GB"));
    }
}
