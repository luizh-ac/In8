//! Administrator rights on Windows: checking the process token and relaunching through UAC.
//! There is no equivalent on Linux on purpose (see PLAN.md §7): root tasks go through `pkexec` per run.

/// Whether In8 runs with administrator rights. Always `false` outside Windows.
pub fn is_elevated() -> bool {
    imp::is_elevated()
}

/// Outcome of asking Windows to start an elevated copy of In8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relaunch {
    /// The elevated copy started; this one should exit.
    Started,
    /// The user said no in the UAC prompt.
    Declined,
}

/// Starts this executable again with administrator rights (one UAC prompt). Arguments are passed through.
pub fn relaunch_elevated(args: &[&str]) -> std::io::Result<Relaunch> {
    imp::relaunch_elevated(args)
}

#[cfg(windows)]
mod imp {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::UI::Shell::{SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    use super::Relaunch;

    pub fn is_elevated() -> bool {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: GetCurrentProcess returns a pseudo-handle; `token` is written by the call and closed below.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut returned = 0u32;
        // SAFETY: the buffer is a TOKEN_ELEVATION and its exact size is passed.
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned,
            )
        };
        // SAFETY: `token` was opened above and is closed exactly once.
        unsafe { CloseHandle(token) };
        ok != 0 && elevation.TokenIsElevated != 0
    }

    fn wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Quotes one argument for the Windows command line (CommandLineToArgvW rules).
    fn quote(arg: &str) -> String {
        if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
            return arg.to_string();
        }
        let mut quoted = String::from('"');
        let mut backslashes = 0;
        for c in arg.chars() {
            match c {
                '\\' => backslashes += 1,
                '"' => {
                    quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                    quoted.push('"');
                    backslashes = 0;
                }
                _ => {
                    quoted.push_str(&"\\".repeat(backslashes));
                    quoted.push(c);
                    backslashes = 0;
                }
            }
        }
        quoted.push_str(&"\\".repeat(backslashes * 2));
        quoted.push('"');
        quoted
    }

    pub fn relaunch_elevated(args: &[&str]) -> std::io::Result<Relaunch> {
        let exe = std::env::current_exe()?;
        let file = wide(exe.as_os_str());
        let verb = wide(OsStr::new("runas"));
        let params = wide(OsStr::new(
            &args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" "),
        ));
        // SAFETY: zeroed is a valid SHELLEXECUTEINFOW (all pointers null, all flags zero).
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOASYNC;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        // SAFETY: every pointer in `info` points into a buffer that outlives the call.
        if unsafe { ShellExecuteExW(&mut info) } != 0 {
            return Ok(Relaunch::Started);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_CANCELLED as i32) {
            Ok(Relaunch::Declined)
        } else {
            Err(error)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::quote;

        #[test]
        fn quotes_like_the_windows_command_line() {
            assert_eq!(quote("--plain"), "--plain");
            assert_eq!(quote("a b"), "\"a b\"");
            assert_eq!(quote("C:\\dir with space\\"), "\"C:\\dir with space\\\\\"");
            assert_eq!(quote("say \"hi\""), "\"say \\\"hi\\\"\"");
            assert_eq!(quote(""), "\"\"");
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Relaunch;

    pub fn is_elevated() -> bool {
        false
    }

    pub fn relaunch_elevated(_args: &[&str]) -> std::io::Result<Relaunch> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "relaunching as administrator is only available on Windows",
        ))
    }
}
