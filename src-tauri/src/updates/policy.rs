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
    if normalized.contains('-') {
        return None;
    }
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

pub fn install_allowed(dictation_busy: bool) -> bool {
    !dictation_busy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_same_older_and_prerelease() {
        assert!(is_newer_stable("0.1.0", "v0.1.1", false));
        assert!(!is_newer_stable("0.1.1", "v0.1.1", false));
        assert!(!is_newer_stable("0.1.1", "v0.1.0", false));
        assert!(!is_newer_stable("0.1.0", "v0.2.0", true));
        assert!(!is_newer_stable("0.1.0", "1.0.0-beta.1", false));
    }

    #[test]
    fn busy_dictation_defers_install() {
        assert!(install_allowed(false));
        assert!(!install_allowed(true));
    }
}
