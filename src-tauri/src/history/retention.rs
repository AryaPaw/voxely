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
    name == "filter-sample.wav" || name.starts_with("filter-preview")
}

pub fn keep_compare_temp_wav(name: &str) -> bool {
    name.starts_with("model-compare.")
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
        let protected_bytes: u64 = list
            .iter()
            .filter(|rec| protected_ids.contains(&rec.id))
            .map(|rec| recording_bytes(audio_root, rec))
            .sum();
        if protected_bytes >= limit {
            return Ok(deleted);
        }
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

fn processing_tmp_keep(known: &[Recording], name: &str) -> bool {
    known.iter().any(|rec| {
        rec.raw_audio_path.as_ref().is_some_and(|raw| {
            let tmp = format!("{raw}.tmp");
            let wav_tmp = Path::new(raw)
                .with_extension("wav.tmp")
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            name == tmp || name == wav_tmp
        })
    })
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
    if !repo.delete(&rec.id)? {
        return Ok(false);
    }
    for rel in [&rec.raw_audio_path, &rec.processed_audio_path]
        .into_iter()
        .flatten()
    {
        let path = audio_root.join(rel);
        if path.exists() {
            if let Err(err) = std::fs::remove_file(&path) {
                tracing::warn!(error = %err, file = %path.display(), "orphan audio after delete");
            }
        }
    }
    Ok(true)
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
            if processing_tmp_keep(&known, &name) {
                continue;
            }
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
    fn cleanup_keeps_filter_and_protected_compare_wavs() {
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
        assert!(!audio.join("model-compare.2.stt.wav").exists());
        assert!(!audio.join("orphan.wav").exists());
        std::fs::write(audio.join("model-compare.2.stt.wav"), b"c").unwrap();
        let mut protected = HashSet::new();
        protected.insert("model-compare.2.stt.wav".into());
        cleanup_orphans_except(audio, &repo, &protected).unwrap();
        assert!(audio.join("model-compare.2.stt.wav").exists());
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

    #[test]
    fn oversized_protected_clip_does_not_wipe_history() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        let audio = dir.path();
        let mut live = new_recording("m".into());
        live.raw_audio_path = Some("live.wav".into());
        std::fs::write(audio.join("live.wav"), vec![0u8; 64]).unwrap();
        repo.insert(&live).unwrap();
        let mut older = new_recording("m".into());
        older.created_at = Utc::now() - Duration::days(1);
        older.updated_at = older.created_at;
        older.raw_audio_path = Some("old.wav".into());
        std::fs::write(audio.join("old.wav"), vec![0u8; 8]).unwrap();
        repo.insert(&older).unwrap();
        let mut protected = HashSet::new();
        protected.insert(live.id.clone());
        let deleted =
            apply_retention_except(&repo, audio, Retention::Forever, Some(16), &protected).unwrap();
        assert!(deleted.is_empty());
        assert!(repo.get(&older.id).unwrap().is_some());
        assert!(audio.join("old.wav").exists());
    }

    #[test]
    fn cleanup_keeps_known_raw_tmp() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        let mut rec = new_recording("m".into());
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        repo.insert(&rec).unwrap();
        let tmp = dir.path().join(format!("{}.raw.wav.tmp", rec.id));
        std::fs::write(&tmp, b"pcm").unwrap();
        cleanup_orphans(dir.path(), &repo).unwrap();
        assert!(tmp.exists());
    }
}
