use std::path::{Path, PathBuf};

pub fn emit_git_dependencies(repo: &Path) {
    if let Some(head) = git_path(repo, "HEAD") {
        println!("cargo:rerun-if-changed={}", head.display());
    }
    if let Some(reference) = git_output(repo, &["symbolic-ref", "--quiet", "HEAD"]) {
        if let Some(path) = git_path(repo, &reference) {
            // A packed reference has no loose file yet. Watch its existing
            // parent so the next commit creating that file also invalidates Cargo.
            if let Some(existing) = path.ancestors().find(|path| path.exists()) {
                println!("cargo:rerun-if-changed={}", existing.display());
            }
        }
        if let Some(packed) = git_path(repo, "packed-refs").filter(|path| path.exists()) {
            println!("cargo:rerun-if-changed={}", packed.display());
        }
    }
}

fn git_path(repo: &Path, name: &str) -> Option<PathBuf> {
    git_output(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-path", name],
    )
    .map(PathBuf::from)
}

fn git_output(repo: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn resolve_build_date(repo: &Path) -> String {
    if let Ok(value) = std::env::var("VOXELY_BUILD_DATE") {
        if is_iso_day(&value) {
            return value;
        }
    }
    if let Ok(epoch) = std::env::var("SOURCE_DATE_EPOCH") {
        if let Ok(secs) = epoch.parse::<i64>() {
            if let Some(day) = unix_day(secs) {
                return day;
            }
        }
    }
    if let Some(day) = git_output(repo, &["log", "-1", "--format=%cs"]) {
        if is_iso_day(&day) {
            return day;
        }
    }
    unix_day(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    )
    .unwrap_or_else(|| "1970-01-01".into())
}

fn is_iso_day(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes.iter().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                true
            } else {
                b.is_ascii_digit()
            }
        })
}

fn unix_day(secs: i64) -> Option<String> {
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u32;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    Some(format!("{y:04}-{m:02}-{d:02}"))
}
