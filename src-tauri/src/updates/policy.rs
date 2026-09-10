use std::path::{Path, PathBuf};

pub const OWNER: &str = "AryaPaw";
pub const REPOSITORY: &str = "voxely";
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
pub const MAX_INSTALLER_BYTES: u64 = 80 * 1024 * 1024;
pub const SETUP_PREFIX: &str = "Voxely-Setup-win-x64-";
pub const PROBE_URL: &str = "https://github.com/AryaPaw/voxely";

pub fn latest_api() -> String {
    format!("https://api.github.com/repos/{OWNER}/{REPOSITORY}/releases/latest")
}

pub fn normalize_version(version: &str) -> String {
    let mut value = version.trim().to_string();
    if let Some(rest) = value.strip_prefix('v').or_else(|| value.strip_prefix('V')) {
        value = rest.to_string();
    }
    if let Some(plus) = value.find('+') {
        value.truncate(plus);
    }
    value
}

pub fn parse_tag(tag: &str) -> Option<(u32, u32, u32)> {
    let normalized = normalize_version(tag);
    let mut parts = normalized.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

pub fn is_newer(current: (u32, u32, u32), candidate: (u32, u32, u32)) -> bool {
    candidate > current
}

pub fn is_newer_stable(current: &str, candidate: &str, prerelease: bool) -> bool {
    if prerelease {
        return false;
    }
    let Some(cur) = parse_tag(current) else {
        return false;
    };
    let Some(next) = parse_tag(candidate) else {
        return false;
    };
    is_newer(cur, next)
}

pub fn allows_background_process(process_name: &str) -> bool {
    let name = process_name.to_ascii_lowercase();
    !name.starts_with("testhost") && !name.starts_with("vstest")
}

pub fn has_inno_uninstaller(application_directory: &Path) -> bool {
    application_directory.join("unins000.exe").is_file()
}

pub fn contains_shell_metacharacters(path: &str) -> bool {
    path.bytes()
        .any(|b| matches!(b, b'&' | b'|' | b'^' | b'%' | b'"' | b'<' | b'>'))
}

pub fn is_inside_root(root: &Path, candidate: &Path) -> bool {
    let root_full = normalize_full(root);
    let candidate_full = normalize_full(candidate);
    candidate_full.starts_with(&root_full)
}

fn normalize_full(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let mut stack = PathBuf::new();
    for component in abs.components() {
        match component {
            std::path::Component::ParentDir => {
                let _ = stack.pop();
            }
            std::path::Component::CurDir => {}
            other => stack.push(other.as_os_str()),
        }
    }
    stack
}

fn https_host_and_path(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    if host.is_empty() {
        return None;
    }
    Some((host.to_ascii_lowercase(), format!("/{path}")))
}

pub fn is_github_release_download(url: &str) -> bool {
    let Some((host, path)) = https_host_and_path(url) else {
        return false;
    };
    if host != "github.com" {
        return false;
    }
    let prefix = format!("/{OWNER}/{REPOSITORY}/releases/download/");
    let path_only = path.split('?').next().unwrap_or(path.as_str());
    path_only.starts_with(&prefix) && !path_only.contains("..") && path_only.len() > prefix.len()
}

pub fn is_allowed_redirect_url(url: &str) -> bool {
    let Some((host, _)) = https_host_and_path(url) else {
        return false;
    };
    if is_allowed_release_cdn_host(&host) {
        return true;
    }
    is_github_release_download(url)
}

pub fn safe_installer_file_name(name: &str) -> Option<String> {
    if name.is_empty() || name.contains(['/', '\\', ':']) || name.contains("..") {
        return None;
    }
    let file = Path::new(name).file_name()?.to_string_lossy();
    if file != name {
        return None;
    }
    let lower = file.to_ascii_lowercase();
    if !file.starts_with(SETUP_PREFIX) || !lower.ends_with(".exe") {
        return None;
    }
    Some(file.into_owned())
}

pub fn is_allowed_release_cdn_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "objects.githubusercontent.com"
            | "release-assets.githubusercontent.com"
            | "github-releases.githubusercontent.com"
    )
}

pub fn is_allowed_asset_url(url: &str) -> bool {
    is_github_release_download(url)
}

pub fn digest_sha256(digest: &str) -> Option<String> {
    let trimmed = digest.trim();
    trimmed
        .strip_prefix("sha256:")
        .or_else(|| trimmed.strip_prefix("SHA256:"))
        .map(|value| value.to_ascii_lowercase())
}

pub fn hashes_match(expected: &str, actual: &str) -> bool {
    let left = digest_sha256(expected).unwrap_or_else(|| expected.trim().to_ascii_lowercase());
    let right = digest_sha256(actual).unwrap_or_else(|| actual.trim().to_ascii_lowercase());
    !left.is_empty() && left == right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_name_must_match_prefix() {
        assert_eq!(
            safe_installer_file_name("Voxely-Setup-win-x64-0.1.1.exe").as_deref(),
            Some("Voxely-Setup-win-x64-0.1.1.exe")
        );
        assert!(safe_installer_file_name("evil.exe").is_none());
        assert!(safe_installer_file_name("../Voxely-Setup-win-x64-1.exe").is_none());
    }

    #[test]
    fn github_302_hosts_are_allowed() {
        assert!(is_allowed_asset_url(
            "https://github.com/AryaPaw/voxely/releases/download/v0.1.1/Voxely-Setup-win-x64-0.1.1.exe"
        ));
        assert!(is_allowed_redirect_url(
            "https://release-assets.githubusercontent.com/123"
        ));
        assert!(!is_allowed_redirect_url("http://evil.example/x"));
    }

    #[test]
    fn newer_stable_tags() {
        assert!(is_newer_stable("0.1.0", "v0.1.1", false));
        assert!(!is_newer_stable("0.1.1", "v0.1.1", false));
        assert!(!is_newer_stable("0.1.0", "v0.2.0", true));
    }

    #[test]
    fn installer_script_keeps_silent_hooks() {
        let text = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../installer/voxely.iss"
        ));
        assert!(text.contains("CloseApplications=no"));
        assert!(text.contains("skipifnotsilent"));
        assert!(text.contains("taskkill"));
    }
}
