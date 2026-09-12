use chrono::{Duration, Utc};
use std::path::Path;

use super::repository::{HistoryRepo, Recording};
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention {
    Days(u32),
    Forever,
}

impl Retention {
    pub fn from_setting(value: &str) -> Self {
        match value {
            "1d" => Retention::Days(1),
            "3d" => Retention::Days(3),
            "7d" => Retention::Days(7),
            "30d" => Retention::Days(30),
            "90d" => Retention::Days(90),
            _ => Retention::Forever,
        }
    }
}

pub fn apply_retention(
    repo: &HistoryRepo,
    audio_root: &Path,
    retention: Retention,
    storage_limit_bytes: Option<u64>,
) -> Result<Vec<String>, AppError> {
    let mut deleted = Vec::new();
    if let Retention::Days(days) = retention {
        let cutoff = Utc::now() - Duration::days(days as i64);
        for rec in repo.older_than(cutoff)? {
            if delete_recording(repo, audio_root, &rec)? {
                deleted.push(rec.id);
            }
        }
    }
    if let Some(limit) = storage_limit_bytes {
        let mut list = repo.list(10_000)?;
        list.sort_by_key(|a| a.created_at);
        while repo.total_audio_bytes(audio_root) > limit {
            let Some(oldest) = list.first().cloned() else {
                break;
            };
            list.remove(0);
            if delete_recording(repo, audio_root, &oldest)? {
                deleted.push(oldest.id);
            } else {
                break;
            }
        }
    }
    Ok(deleted)
}

pub fn delete_recording(
    repo: &HistoryRepo,
    audio_root: &Path,
    rec: &Recording,
) -> Result<bool, AppError> {
    for rel in [&rec.raw_audio_path, &rec.processed_audio_path]
        .into_iter()
        .flatten()
    {
        let path = audio_root.join(rel);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        }
    }
    repo.delete(&rec.id)?;
    Ok(true)
}

pub fn cleanup_orphans(audio_root: &Path, repo: &HistoryRepo) -> Result<(), AppError> {
    let known = repo.list(20_000)?;
    let mut keep = std::collections::HashSet::new();
    for rec in &known {
        if let Some(p) = &rec.raw_audio_path {
            keep.insert(p.clone());
        }
        if let Some(p) = &rec.processed_audio_path {
            keep.insert(p.clone());
        }
    }
    if !audio_root.exists() {
        return Ok(());
    }
    for entry in
        std::fs::read_dir(audio_root).map_err(|e| AppError::StorageFailed(e.to_string()))?
    {
        let entry = entry.map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".tmp") || name.ends_with(".wav.tmp") {
            let _ = std::fs::remove_file(entry.path());
            continue;
        }
        if !keep.contains(&name)
            && name.ends_with(".wav")
            && !crate::app::compare::keep_compare_wav(&name)
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::repository::new_recording;
    use tempfile::tempdir;

    #[test]
    fn does_not_delete_new_records() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        let rec = new_recording("m".into());
        repo.insert(&rec).unwrap();
        let deleted = apply_retention(&repo, dir.path(), Retention::Days(7), None).unwrap();
        assert!(deleted.is_empty());
        assert_eq!(repo.list(10).unwrap().len(), 1);
    }

    #[test]
    fn cleanup_keeps_filter_and_compare_wavs() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        let audio = dir.path();
        std::fs::write(audio.join("filter-sample.wav"), b"a").unwrap();
        std::fs::write(audio.join("filter-preview.wav"), b"b").unwrap();
        std::fs::write(audio.join("model-compare.stt.wav"), b"c").unwrap();
        std::fs::write(audio.join("orphan.wav"), b"d").unwrap();
        cleanup_orphans(audio, &repo).unwrap();
        assert!(audio.join("filter-sample.wav").exists());
        assert!(audio.join("filter-preview.wav").exists());
        assert!(audio.join("model-compare.stt.wav").exists());
        assert!(!audio.join("orphan.wav").exists());
    }

    #[test]
    fn three_day_setting_parses() {
        assert_eq!(Retention::from_setting("3d"), Retention::Days(3));
    }
}
