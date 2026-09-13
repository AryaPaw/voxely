use std::fs;
use std::io;
use std::path::Path;

use tracing_appender::rolling::{RollingFileAppender, Rotation};

pub const MAX_LOG_FILES: usize = 14;
pub const LOG_PREFIX: &str = "voxely";
pub const LOG_SUFFIX: &str = "log";

pub fn migrate_legacy_log_names(dir: &Path) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(date) = name.strip_prefix("voxely.log.") else {
            continue;
        };
        if date.len() != 10 || date.as_bytes()[4] != b'-' || date.as_bytes()[7] != b'-' {
            continue;
        }
        if !date.chars().all(|c| c.is_ascii_digit() || c == '-') {
            continue;
        }
        let dest = dir.join(format!("{LOG_PREFIX}.{date}.{LOG_SUFFIX}"));
        if dest.exists() {
            let _ = fs::remove_file(entry.path());
            continue;
        }
        fs::rename(entry.path(), dest)?;
    }
    Ok(())
}

pub fn file_appender(dir: &Path) -> io::Result<RollingFileAppender> {
    fs::create_dir_all(dir)?;
    migrate_legacy_log_names(dir)?;
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_PREFIX)
        .filename_suffix(LOG_SUFFIX)
        .max_log_files(MAX_LOG_FILES)
        .build(dir)
        .map_err(|err| io::Error::other(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn migrates_dated_extension_to_log_suffix() {
        let dir = tempdir().unwrap();
        let old = dir.path().join("voxely.log.2026-09-13");
        fs::write(&old, "hello").unwrap();
        migrate_legacy_log_names(dir.path()).unwrap();
        let renamed = dir.path().join("voxely.2026-09-13.log");
        assert!(renamed.exists());
        assert!(!old.exists());
        assert_eq!(fs::read_to_string(renamed).unwrap(), "hello");
    }

    #[test]
    fn keeps_current_log_if_both_names_exist() {
        let dir = tempdir().unwrap();
        let old = dir.path().join("voxely.log.2026-09-13");
        let current = dir.path().join("voxely.2026-09-13.log");
        fs::write(&old, "old").unwrap();
        fs::write(&current, "new").unwrap();
        migrate_legacy_log_names(dir.path()).unwrap();
        assert!(current.exists());
        assert!(!old.exists());
        assert_eq!(fs::read_to_string(current).unwrap(), "new");
    }
}
