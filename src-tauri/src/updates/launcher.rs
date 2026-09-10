use super::policy::{contains_shell_metacharacters, safe_installer_file_name};

pub fn silent_cmd_tail(setup_path: &str) -> Option<String> {
    if setup_path.trim().is_empty()
        || contains_shell_metacharacters(setup_path)
        || !setup_path.to_ascii_lowercase().ends_with(".exe")
    {
        return None;
    }
    let name = std::path::Path::new(setup_path)
        .file_name()?
        .to_string_lossy();
    safe_installer_file_name(&name)?;
    let quoted = std::path::Path::new(setup_path).to_string_lossy();
    Some(format!(
        "ping 127.0.0.1 -n 5 >NUL & \"{quoted}\" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /FORCECLOSEAPPLICATIONS"
    ))
}

#[cfg(windows)]
pub fn try_start_silent(setup_path: &str) -> bool {
    let Some(tail) = silent_cmd_tail(setup_path) else {
        return false;
    };
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let cmd = std::path::Path::new(&system)
        .join("System32")
        .join("cmd.exe");
    use std::os::windows::process::CommandExt;
    std::process::Command::new(cmd)
        .arg("/C")
        .arg(tail)
        .creation_flags(0x08000000)
        .spawn()
        .is_ok()
}

#[cfg(not(windows))]
pub fn try_start_silent(_setup_path: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_setup_path() {
        assert!(silent_cmd_tail("Voxely-Setup-win-x64-1.exe & calc").is_none());
        assert!(silent_cmd_tail("not-an-installer.exe").is_none());
    }

    #[test]
    fn builds_verysilent_command() {
        let tail = silent_cmd_tail(r"C:\Temp\Voxely-Setup-win-x64-0.1.1.exe").unwrap();
        assert!(tail.contains("/VERYSILENT"));
        assert!(tail.contains("Voxely-Setup-win-x64-0.1.1.exe"));
    }
}
