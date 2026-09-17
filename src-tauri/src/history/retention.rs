use chrono::{Duration, Utc};
use std::collections::HashSet;
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

pub fn keep_temp_wav(name: &str) -> bool {
    name == "filter-sample.wav"
        || name.starts_with("filter-preview")
        || name.starts_with("model-compare.")
}

pub fn apply_retention(
    repo: &HistoryRepo,
    audio_root: &Path,
    retention: Retention,
    storage_limit_bytes: Option<u64>,
) -> Result<Vec<String>, AppError> {
    apply_retention_except(
        repo,
        audio_root,
        retention,
        storage_limit_bytes,
        &HashSet::new(),
    )
}

pub fn apply_retention_except(
    repo: &HistoryRepo,
    audio_root: &Path,
    retention: Retention,
    storage_limit_bytes: Option<u64>,
    protected_ids: &HashSet<String>,
) -> Result<Vec<String>, AppError> {
    let mut deleted = Vec::new();
    if let Retention::Days(days) = retention {
        let cutoff = Utc::now() - Duration::days(days as i64);
        for rec in repo.older_than(cutoff)? {
            if protected_ids.contains(&rec.id) {
                continue;
            }
            match delete_recording(repo, audio_root, &rec) {
                Ok(true) => deleted.push(rec.id),
                Ok(false) => {}
                Err(err) => tracing::warn!(error = %err, id = %rec.id, "retention skip"),
            }
        }
    }
    if let Some(limit) = storage_limit_bytes {
        let mut list = repo.list_all()?;
        list.sort_by_key(|a| a.created_at);
        let mut bytes = inventory_bytes(audio_root, &list);
        for rec in list {
            if bytes <= limit {
                break;
            }
            if protected_ids.contains(&rec.id) {
                continue;
            }
            let rec_bytes = recording_bytes(audio_root, &rec);
            match delete_recording(repo, audio_root, &rec) {
                Ok(true) => {
                    deleted.push(rec.id);
                    bytes = bytes.saturating_sub(rec_bytes);
                }
                Ok(false) => {}
                Err(err) => {
                    tracing::warn!(error = %err, id = %rec.id, "retention skip");
                    break;
                }
            }
        }
    }
    Ok(deleted)
}

fn inventory_bytes(audio_root: &Path, list: &[Recording]) -> u64 {
    list.iter()
        .map(|rec| recording_bytes(audio_root, rec))
        .sum()
}

fn recording_bytes(audio_root: &Path, rec: &Recording) -> u64 {
    [&rec.raw_audio_path, &rec.processed_audio_path]
        .into_iter()
        .flatten()
        .map(|path| {
            std::fs::metadata(audio_root.join(path))
                .map(|meta| meta.len())
                .unwrap_or(0)
        })
        .sum()
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
    repo.delete(&rec.id)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAllResult {
    pub deleted: Vec<String>,
    pub failed: Vec<String>,
}

use serde::{Deserialize, Serialize};

pub fn delete_all_recordings(
    repo: &HistoryRepo,
    audio_root: &Path,
    protected_ids: &HashSet<String>,
) -> Result<DeleteAllResult, AppError> {
    let mut result = DeleteAllResult {
        deleted: Vec::new(),
        failed: Vec::new(),
    };
    for rec in repo.list_all()? {
        if protected_ids.contains(&rec.id) {
            result.failed.push(rec.id);
            continue;
        }
        match delete_recording(repo, audio_root, &rec) {
            Ok(true) => result.deleted.push(rec.id),
            Ok(false) => result.failed.push(rec.id),
            Err(_) => result.failed.push(rec.id),
        }
    }
    Ok(result)
}

pub fn cleanup_orphans(audio_root: &Path, repo: &HistoryRepo) -> Result<(), AppError> {
    cleanup_orphans_except(audio_root, repo, &HashSet::new())
}

pub fn cleanup_orphans_except(
    audio_root: &Path,
    repo: &HistoryRepo,
    protected_names: &HashSet<String>,
) -> Result<(), AppError> {
    let known = repo.list_all()?;
    let mut keep = HashSet::new();
    for rec in &known {
        if let Some(p) = &rec.raw_audio_path {
            keep.insert(p.clone());
        }
        if let Some(p) = &rec.processed_audio_path {
            keep.insert(p.clone());
        }
    }
    keep.extend(protected_names.iter().cloned());
    if !audio_root.exists() {
        return Ok(());
    }
    for entry in
        std::fs::read_dir(audio_root).map_err(|e| AppError::StorageFailed(e.to_string()))?
    {
        let entry = entry.map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if protected_names.contains(&name) {
            continue;
        }
        if name.ends_with(".tmp") || name.ends_with(".wav.tmp") {
            let _ = std::fs::remove_file(entry.path());
            continue;
        }
        if !keep.contains(&name) && name.ends_with(".wav") && !keep_temp_wav(&name) {
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
        std::fs::write(audio.join("model-compare.2.stt.wav"), b"c").unwrap();
        std::fs::write(audio.join("orphan.wav"), b"d").unwrap();
        cleanup_orphans(audio, &repo).unwrap();
        assert!(audio.join("filter-sample.wav").exists());
        assert!(audio.join("filter-preview.wav").exists());
        assert!(audio.join("model-compare.2.stt.wav").exists());
        assert!(!audio.join("orphan.wav").exists());
    }

    #[test]
    fn protected_id_survives_retention() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        let mut rec = new_recording("m".into());
        rec.created_at = Utc::now() - Duration::days(30);
        rec.updated_at = rec.created_at;
        repo.insert(&rec).unwrap();
        let mut protected = HashSet::new();
        protected.insert(rec.id.clone());
        let deleted =
            apply_retention_except(&repo, dir.path(), Retention::Days(1), None, &protected)
                .unwrap();
        assert!(deleted.is_empty());
        assert!(repo.get(&rec.id).unwrap().is_some());
    }

    #[test]
    fn three_day_setting_parses() {
        assert_eq!(Retention::from_setting("3d"), Retention::Days(3));
    }
}
