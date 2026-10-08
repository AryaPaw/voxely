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
    apply_retention_with_result(
        repo,
        audio_root,
        retention,
        storage_limit_bytes,
        protected_ids,
        &HashSet::new(),
    )
    .map(|result| result.deleted)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPreview {
    pub entries_to_delete: usize,
    pub recording_ids_to_delete: Vec<String>,
    pub files_to_delete: usize,
    pub bytes_to_free: u64,
    pub protected_entries: usize,
    pub protected_bytes: u64,
    pub total_audio_bytes: u64,
}

struct RetentionPlan {
    selected: Vec<Recording>,
    protected_entries: usize,
    protected_bytes: u64,
    total_audio_bytes: u64,
}

fn plan_retention(
    repo: &HistoryRepo,
    audio_root: &Path,
    retention: Retention,
    storage_limit_bytes: Option<u64>,
    protected_ids: &HashSet<String>,
    protected_names: &HashSet<String>,
) -> Result<RetentionPlan, AppError> {
    let mut recordings = repo.list_all()?;
    // Cancelled tombstones own a separate retry path because their deterministic audio may
    // still exist after a failed unlink. Retention must not discard the only cleanup ID.
    recordings.retain(|rec| rec.last_error_code.as_deref() != Some("Cancelled"));
    recordings.sort_by_key(|rec| rec.created_at);
    let cutoff = match retention {
        Retention::Days(days) => Some(Utc::now() - Duration::days(days as i64)),
        Retention::Forever => None,
    };
    let is_protected =
        |rec: &Recording| is_protected_recording(rec, protected_ids, protected_names);
    let protected: Vec<_> = recordings.iter().filter(|rec| is_protected(rec)).collect();
    let mut protected_paths: HashSet<String> = protected
        .iter()
        .flat_map(|rec| [&rec.raw_audio_path, &rec.processed_audio_path])
        .flatten()
        .filter(|name| safe_audio_path(audio_root, name).is_some())
        .cloned()
        .collect();
    protected_paths.extend(protected_names.iter().filter_map(|name| {
        let path = safe_audio_path(audio_root, name)?;
        path.is_file().then(|| name.clone())
    }));
    let protected_bytes = file_names_bytes(audio_root, &protected_paths);
    let total_audio_bytes = audio_inventory_bytes(audio_root);
    let mut planned_ids = HashSet::new();
    let mut selected = Vec::new();
    let mut planned_bytes = 0u64;

    if let Some(cutoff) = cutoff {
        for rec in &recordings {
            if rec.created_at < cutoff && !is_protected(rec) && planned_ids.insert(rec.id.clone()) {
                planned_bytes = planned_bytes.saturating_add(recording_bytes(audio_root, rec));
                selected.push(rec.clone());
            }
        }
    }

    if let Some(limit) = storage_limit_bytes {
        let mut remaining = total_audio_bytes.saturating_sub(planned_bytes);
        if protected_bytes < limit && remaining > limit {
            for rec in &recordings {
                if remaining <= limit {
                    break;
                }
                if is_protected(rec) || !planned_ids.insert(rec.id.clone()) {
                    continue;
                }
                let bytes = recording_bytes(audio_root, rec);
                if bytes == 0 {
                    continue;
                }
                planned_bytes = planned_bytes.saturating_add(bytes);
                remaining = remaining.saturating_sub(bytes);
                selected.push(rec.clone());
            }
        }
    }

    Ok(RetentionPlan {
        selected,
        protected_entries: protected.len(),
        protected_bytes,
        total_audio_bytes,
    })
}

pub fn preview_retention(
    repo: &HistoryRepo,
    audio_root: &Path,
    retention: Retention,
    storage_limit_bytes: Option<u64>,
    protected_ids: &HashSet<String>,
    protected_names: &HashSet<String>,
) -> Result<RetentionPreview, AppError> {
    let plan = plan_retention(
        repo,
        audio_root,
        retention,
        storage_limit_bytes,
        protected_ids,
        protected_names,
    )?;
    let files: HashSet<_> = plan
        .selected
        .iter()
        .flat_map(|rec| [&rec.raw_audio_path, &rec.processed_audio_path])
        .flatten()
        .filter(|name| safe_audio_path(audio_root, name).is_some_and(|path| path.is_file()))
        .cloned()
        .collect();
    let bytes_to_free = files
        .iter()
        .map(|name| {
            safe_audio_path(audio_root, name)
                .and_then(|path| std::fs::metadata(path).ok())
                .map(|meta| meta.len())
                .unwrap_or(0)
        })
        .sum();
    Ok(RetentionPreview {
        entries_to_delete: plan.selected.len(),
        recording_ids_to_delete: plan.selected.iter().map(|rec| rec.id.clone()).collect(),
        files_to_delete: files.len(),
        bytes_to_free,
        protected_entries: plan.protected_entries,
        protected_bytes: plan.protected_bytes,
        total_audio_bytes: plan.total_audio_bytes,
    })
}

pub fn apply_retention_recording_ids(
    repo: &HistoryRepo,
    audio_root: &Path,
    recording_ids: &[String],
    protected_ids: &HashSet<String>,
    protected_names: &HashSet<String>,
) -> Result<DeleteAllResult, AppError> {
    let mut recordings = Vec::with_capacity(recording_ids.len());
    for id in recording_ids {
        let recording = repo.get(id)?.ok_or_else(|| {
            AppError::RequestValidationFailed(
                "retention preview changed; review the updated preview before applying".into(),
            )
        })?;
        if is_protected_recording(&recording, protected_ids, protected_names) {
            return Err(AppError::RequestValidationFailed(
                "retention preview changed; review the updated preview before applying".into(),
            ));
        }
        recordings.push(recording);
    }

    let mut result = DeleteAllResult {
        deleted: Vec::new(),
        failed: Vec::new(),
    };
    for recording in recordings {
        match delete_recording(repo, audio_root, &recording) {
            Ok(true) => result.deleted.push(recording.id),
            Ok(false) => result.failed.push(recording.id),
            Err(err) => {
                tracing::warn!(error = %err, id = %recording.id, "retention skip");
                result.failed.push(recording.id);
            }
        }
    }
    Ok(result)
}

fn is_protected_recording(
    recording: &Recording,
    protected_ids: &HashSet<String>,
    protected_names: &HashSet<String>,
) -> bool {
    protected_ids.contains(&recording.id)
        || [&recording.raw_audio_path, &recording.processed_audio_path]
            .into_iter()
            .flatten()
            .filter_map(|name| Path::new(name).file_name())
            .any(|name| protected_names.contains(&name.to_string_lossy().into_owned()))
}

pub fn apply_retention_with_result(
    repo: &HistoryRepo,
    audio_root: &Path,
    retention: Retention,
    storage_limit_bytes: Option<u64>,
    protected_ids: &HashSet<String>,
    protected_names: &HashSet<String>,
) -> Result<DeleteAllResult, AppError> {
    let plan = plan_retention(
        repo,
        audio_root,
        retention,
        storage_limit_bytes,
        protected_ids,
        protected_names,
    )?;
    let mut result = DeleteAllResult {
        deleted: Vec::new(),
        failed: Vec::new(),
    };
    for rec in plan.selected {
        match delete_recording(repo, audio_root, &rec) {
            Ok(true) => result.deleted.push(rec.id),
            Ok(false) => result.failed.push(rec.id),
            Err(err) => {
                tracing::warn!(error = %err, id = %rec.id, "retention skip");
                result.failed.push(rec.id);
            }
        }
    }
    Ok(result)
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

fn audio_inventory_bytes(audio_root: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(audio_root) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let metadata = entry.metadata().ok()?;
            (metadata.is_file() && (name.ends_with(".wav") || name.ends_with(".wav.tmp")))
                .then_some(metadata.len())
        })
        .sum()
}

fn file_names_bytes(audio_root: &Path, names: &HashSet<String>) -> u64 {
    names
        .iter()
        .map(|name| {
            safe_audio_path(audio_root, name)
                .and_then(|path| std::fs::metadata(path).ok())
                .filter(|meta| meta.is_file())
                .map(|meta| meta.len())
                .unwrap_or(0)
        })
        .sum()
}

fn safe_audio_path(audio_root: &Path, name: &str) -> Option<std::path::PathBuf> {
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(file_name)), None)
            if !file_name.is_empty() && file_name.to_string_lossy() == name =>
        {
            Some(audio_root.join(file_name))
        }
        _ => None,
    }
}

fn recording_bytes(audio_root: &Path, rec: &Recording) -> u64 {
    [&rec.raw_audio_path, &rec.processed_audio_path]
        .into_iter()
        .flatten()
        .map(|path| {
            safe_audio_path(audio_root, path)
                .and_then(|path| std::fs::metadata(path).ok())
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
    delete_recording_with_staged_audio(repo, audio_root, rec, |repo, id| repo.delete(id))
}

pub fn delete_recording_and_usage(
    repo: &HistoryRepo,
    audio_root: &Path,
    rec: &Recording,
) -> Result<bool, AppError> {
    delete_recording_with_staged_audio(repo, audio_root, rec, |repo, id| repo.delete_with_usage(id))
}

fn delete_recording_with_staged_audio(
    repo: &HistoryRepo,
    audio_root: &Path,
    rec: &Recording,
    delete: impl FnOnce(&HistoryRepo, &str) -> Result<bool, AppError>,
) -> Result<bool, AppError> {
    let staged = stage_recording_audio_for_delete(audio_root, rec)?;
    let deleted = match delete(repo, &rec.id) {
        Ok(deleted) => deleted,
        Err(err) => {
            restore_staged_recording_audio(&staged)?;
            return Err(err);
        }
    };
    if !deleted {
        restore_staged_recording_audio(&staged)?;
        return Ok(false);
    }

    let mut cleanup_failures = Vec::new();
    for (_, staged_path) in staged {
        if let Err(err) = std::fs::remove_file(&staged_path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(error = %err, file = %staged_path.display(), "remove staged recording audio after delete");
                cleanup_failures.push(format!("{}: {err}", staged_path.display()));
            }
        }
    }
    if !cleanup_failures.is_empty() {
        return Err(AppError::StorageFailed(format!(
            "history was deleted, but audio cleanup is pending: {}",
            cleanup_failures.join("; ")
        )));
    }
    Ok(true)
}

fn stage_recording_audio_for_delete(
    audio_root: &Path,
    rec: &Recording,
) -> Result<Vec<(std::path::PathBuf, std::path::PathBuf)>, AppError> {
    let mut staged = Vec::new();
    for relative_path in [&rec.raw_audio_path, &rec.processed_audio_path]
        .into_iter()
        .flatten()
    {
        let Some(original_path) = safe_audio_path(audio_root, relative_path) else {
            restore_staged_recording_audio(&staged)?;
            return Err(AppError::StorageFailed(format!(
                "invalid recording audio path: {relative_path}"
            )));
        };
        let metadata = match std::fs::symlink_metadata(&original_path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                restore_staged_recording_audio(&staged)?;
                return Err(AppError::StorageFailed(format!(
                    "inspect recording audio for delete: {err}"
                )));
            }
        };
        if !metadata.file_type().is_file() && !metadata.file_type().is_symlink() {
            restore_staged_recording_audio(&staged)?;
            return Err(AppError::StorageFailed(format!(
                "recording audio path is not a file: {}",
                original_path.display()
            )));
        }
        let staged_path = staged_audio_path(&original_path);
        if let Err(err) = std::fs::rename(&original_path, &staged_path) {
            restore_staged_recording_audio(&staged)?;
            return Err(AppError::StorageFailed(format!(
                "stage recording audio for delete: {err}"
            )));
        }
        staged.push((original_path, staged_path));
    }
    Ok(staged)
}

fn staged_audio_path(original_path: &Path) -> std::path::PathBuf {
    let name = original_path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    original_path.with_file_name(format!(
        "{name}.voxely-delete-pending-{}",
        uuid::Uuid::new_v4()
    ))
}

fn restore_staged_recording_audio(
    staged: &[(std::path::PathBuf, std::path::PathBuf)],
) -> Result<(), AppError> {
    let mut failures = Vec::new();
    for (original_path, staged_path) in staged.iter().rev() {
        if !staged_path.exists() {
            continue;
        }
        if original_path.exists() {
            failures.push(format!(
                "{}: destination already exists",
                original_path.display()
            ));
            continue;
        }
        if let Err(err) = std::fs::rename(staged_path, original_path) {
            failures.push(format!("{}: {err}", original_path.display()));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(AppError::StorageFailed(format!(
            "restore recording audio after failed delete: {}",
            failures.join("; ")
        )))
    }
}

pub fn delete_cancelled_recording(
    repo: &HistoryRepo,
    audio_root: &Path,
    id: &str,
) -> Result<bool, AppError> {
    let Some(rec) = repo.get(id)? else {
        return Ok(false);
    };
    delete_recording_and_usage(repo, audio_root, &rec)
}

pub fn cleanup_cancelled_audio(audio_root: &Path, id: &str) -> Result<(), AppError> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Ok(());
    }
    let mut failures = Vec::new();
    for name in [
        format!("{id}.raw.wav"),
        format!("{id}.processed.wav"),
        format!("{id}.raw.stt.wav"),
        format!("{id}.processed.stt.wav"),
        format!("{id}.raw.wav.tmp"),
        format!("{id}.processed.wav.tmp"),
        format!("{id}.raw.stt.wav.tmp"),
        format!("{id}.processed.stt.wav.tmp"),
    ] {
        remove_cancelled_audio_file(&audio_root.join(name), &mut failures);
    }
    match std::fs::read_dir(audio_root) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|err| AppError::StorageFailed(err.to_string()))?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with(&format!("{id}.")) && name.contains(".voxely-delete-pending-") {
                    remove_cancelled_audio_file(&entry.path(), &mut failures);
                }
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(AppError::StorageFailed(err.to_string())),
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(AppError::StorageFailed(failures.join("; ")))
    }
}

fn remove_cancelled_audio_file(path: &Path, failures: &mut Vec<String>) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            tracing::warn!(error = %err, file = %path.display(), "remove cancelled audio artifact");
            failures.push(format!("{}: {err}", path.display()));
        }
    }
}

pub fn cleanup_staged_audio_deletions(
    audio_root: &Path,
    repo: &HistoryRepo,
) -> Result<(), AppError> {
    let known = repo.list_all()?;
    let keep: HashSet<String> = known
        .iter()
        .flat_map(|rec| [rec.raw_audio_path.clone(), rec.processed_audio_path.clone()])
        .flatten()
        .collect();
    if !audio_root.exists() {
        return Ok(());
    }
    let mut failures = Vec::new();
    for entry in
        std::fs::read_dir(audio_root).map_err(|err| AppError::StorageFailed(err.to_string()))?
    {
        let entry = entry.map_err(|err| AppError::StorageFailed(err.to_string()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(original_name) = staged_audio_original_name(&name) else {
            continue;
        };
        let original_path = audio_root.join(original_name);
        if keep.contains(original_name) {
            if original_path.exists() {
                failures.push(format!(
                    "cannot recover staged audio {} because {} already exists",
                    entry.path().display(),
                    original_path.display()
                ));
            } else if let Err(err) = std::fs::rename(entry.path(), &original_path) {
                failures.push(format!(
                    "could not restore staged audio {}: {err}",
                    entry.path().display()
                ));
            }
        } else if let Err(err) = std::fs::remove_file(entry.path()) {
            if err.kind() != std::io::ErrorKind::NotFound {
                failures.push(format!(
                    "could not remove staged audio {}: {err}",
                    entry.path().display()
                ));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(AppError::StorageFailed(failures.join("; ")))
    }
}

pub fn remove_filter_artifacts(audio_root: &Path) -> Vec<String> {
    let mut names = vec![
        "filter-sample.wav".to_string(),
        "filter-sample.wav.tmp".to_string(),
        "filter-preview.wav".to_string(),
        "filter-preview.wav.tmp".to_string(),
        "filter-preview-original.wav".to_string(),
        "filter-preview-original.wav.tmp".to_string(),
    ];
    let mut failures = Vec::new();
    match std::fs::read_dir(audio_root) {
        Ok(entries) => {
            for entry in entries {
                let Ok(entry) = entry else {
                    failures.push(audio_root.to_string_lossy().into_owned());
                    continue;
                };
                let name = entry.file_name().to_string_lossy().into_owned();
                if generated_filter_preview_name(&name) {
                    names.push(name);
                }
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            tracing::warn!(error = %err, path = %audio_root.display(), "list filter artifacts");
            failures.push(audio_root.to_string_lossy().into_owned());
        }
    }
    failures.extend(names
    .into_iter()
    .filter_map(|name| {
        let path = audio_root.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => None,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                tracing::warn!(error = %err, file = %path.display(), "remove filter artifact");
                Some(path.to_string_lossy().into_owned())
            }
        }
    })
    .collect::<Vec<_>>());
    failures
}

fn generated_filter_preview_name(name: &str) -> bool {
    let stem = name
        .strip_suffix(".wav.tmp")
        .or_else(|| name.strip_suffix(".wav"));
    let Some(stem) = stem.and_then(|stem| stem.strip_prefix("filter-preview-")) else {
        return false;
    };
    let uuid = stem.strip_suffix("-original").unwrap_or(stem);
    uuid::Uuid::parse_str(uuid).is_ok()
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
        if rec.last_error_code.as_deref() == Some("Cancelled") {
            match cleanup_cancelled_audio(audio_root, &rec.id) {
                Ok(()) => match repo.discard_cancelled_recording(&rec.id) {
                    Ok(_) => result.deleted.push(rec.id),
                    Err(err) => {
                        tracing::warn!(error = %err, recording_id = rec.id, "delete cancelled tombstone during delete-all");
                        result.failed.push(rec.id);
                    }
                },
                Err(err) => {
                    tracing::warn!(error = %err, recording_id = rec.id, "delete-all cancelled audio cleanup remains pending");
                    result.failed.push(rec.id);
                }
            }
            continue;
        }
        match delete_recording(repo, audio_root, &rec) {
            Ok(true) => result.deleted.push(rec.id),
            Ok(false) => result.failed.push(rec.id),
            Err(err) => {
                if repo.get(&rec.id)?.is_none() {
                    result.deleted.push(rec.id);
                    result.failed.push(err.to_string());
                } else {
                    result.failed.push(rec.id);
                }
            }
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
        if let Some(original_name) = staged_audio_original_name(&name) {
            let original_path = audio_root.join(original_name);
            if keep.contains(original_name) {
                if original_path.exists() {
                    return Err(AppError::StorageFailed(format!(
                        "cannot recover staged audio {} because {} already exists",
                        entry.path().display(),
                        original_path.display()
                    )));
                }
                if let Err(err) = std::fs::rename(entry.path(), &original_path) {
                    tracing::warn!(error = %err, file = %entry.path().display(), "restore staged audio during orphan cleanup");
                    return Err(AppError::StorageFailed(format!(
                        "could not restore staged audio {}: {err}",
                        entry.path().display()
                    )));
                }
            } else if !protected_names.contains(&name) {
                if let Err(err) = std::fs::remove_file(entry.path()) {
                    if err.kind() != std::io::ErrorKind::NotFound {
                        return Err(AppError::StorageFailed(format!(
                            "could not remove staged audio {}: {err}",
                            entry.path().display()
                        )));
                    }
                }
            }
            continue;
        }
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

fn staged_audio_original_name(name: &str) -> Option<&str> {
    let (original_name, token) = name.split_once(".voxely-delete-pending-")?;
    if original_name.ends_with(".wav") && !token.is_empty() {
        Some(original_name)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::repository::{new_recording, TranscriptionAttempt};
    use rusqlite::Connection;
    use tempfile::tempdir;

    #[test]
    fn retention_preview_serializes_with_camel_case_wire_fields() {
        let preview = RetentionPreview {
            entries_to_delete: 1,
            recording_ids_to_delete: vec!["recording-1".into()],
            files_to_delete: 2,
            bytes_to_free: 3,
            protected_entries: 4,
            protected_bytes: 5,
            total_audio_bytes: 6,
        };

        let value = serde_json::to_value(preview).unwrap();

        assert_eq!(value["entriesToDelete"], 1);
        assert_eq!(value["recordingIdsToDelete"][0], "recording-1");
        assert_eq!(value["filesToDelete"], 2);
        assert_eq!(value["bytesToFree"], 3);
        assert_eq!(value["protectedEntries"], 4);
        assert_eq!(value["protectedBytes"], 5);
        assert_eq!(value["totalAudioBytes"], 6);
        assert!(value.get("entries_to_delete").is_none());
    }

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
    fn retention_preserves_cancelled_cleanup_tombstone_and_audio() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        let mut rec = new_recording("m".into());
        rec.created_at = Utc::now() - Duration::days(30);
        rec.updated_at = rec.created_at;
        rec.last_error_code = Some("Cancelled".into());
        let audio = dir.path().join(format!("{}.raw.stt.wav", rec.id));
        std::fs::write(&audio, b"pending cancelled audio cleanup").unwrap();
        repo.insert(&rec).unwrap();
        repo.suppress_cancelled_recording(&rec.id).unwrap();

        let deleted = apply_retention(&repo, dir.path(), Retention::Days(1), Some(0)).unwrap();

        assert!(deleted.is_empty());
        assert_eq!(repo.count(None).unwrap(), 0);
        assert!(repo.get(&rec.id).unwrap().is_some());
        assert!(audio.exists());
    }

    #[test]
    fn delete_all_cleans_cancelled_audio_before_removing_tombstone() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        let mut rec = new_recording("m".into());
        rec.last_error_code = Some("Cancelled".into());
        repo.insert(&rec).unwrap();
        repo.suppress_cancelled_recording(&rec.id).unwrap();
        let audio = dir.path().join(format!("{}.raw.stt.wav", rec.id));
        std::fs::write(&audio, b"cancelled audio").unwrap();

        let result = delete_all_recordings(&repo, dir.path(), &HashSet::new()).unwrap();

        assert_eq!(result.deleted, vec![rec.id.clone()]);
        assert!(result.failed.is_empty());
        assert!(repo.get(&rec.id).unwrap().is_none());
        assert!(!audio.exists());
    }

    #[test]
    fn delete_all_keeps_cancelled_tombstone_when_audio_cleanup_fails() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        let mut rec = new_recording("m".into());
        rec.last_error_code = Some("Cancelled".into());
        repo.insert(&rec).unwrap();
        repo.suppress_cancelled_recording(&rec.id).unwrap();
        let audio = dir.path().join(format!("{}.raw.stt.wav", rec.id));
        std::fs::create_dir(&audio).unwrap();

        let result = delete_all_recordings(&repo, dir.path(), &HashSet::new()).unwrap();

        assert!(result.deleted.is_empty());
        assert_eq!(result.failed, vec![rec.id.clone()]);
        assert_eq!(
            repo.get(&rec.id)
                .unwrap()
                .unwrap()
                .last_error_code
                .as_deref(),
            Some("Cancelled")
        );
        assert!(audio.is_dir());
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

    #[test]
    fn delete_failure_preserves_record_and_missing_file_retry_succeeds() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        let mut rec = new_recording("m".into());
        rec.raw_audio_path = Some("locked.wav".into());
        std::fs::create_dir(dir.path().join("locked.wav")).unwrap();
        repo.insert(&rec).unwrap();

        assert!(delete_recording(&repo, dir.path(), &rec).is_err());
        assert!(repo.get(&rec.id).unwrap().is_some());

        std::fs::remove_dir(dir.path().join("locked.wav")).unwrap();
        assert!(delete_recording(&repo, dir.path(), &rec).unwrap());
        assert!(repo.get(&rec.id).unwrap().is_none());
    }

    #[test]
    fn retention_sqlite_failure_restores_audio_and_keeps_record() {
        let dir = tempdir().unwrap();
        let database = dir.path().join("h.db");
        let repo = HistoryRepo::open(&database).unwrap();
        let mut rec = new_recording("m".into());
        rec.created_at = Utc::now() - Duration::days(30);
        rec.updated_at = rec.created_at;
        rec.raw_audio_path = Some("expired.wav".into());
        rec.processed_audio_path = Some("expired-processed.wav".into());
        let raw_audio = dir.path().join("expired.wav");
        let processed_audio = dir.path().join("expired-processed.wav");
        std::fs::write(&raw_audio, b"raw").unwrap();
        std::fs::write(&processed_audio, b"processed").unwrap();
        repo.insert(&rec).unwrap();
        Connection::open(&database)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER reject_recording_delete BEFORE DELETE ON recordings \
                 BEGIN SELECT RAISE(ABORT, 'injected retention delete failure'); END;",
            )
            .unwrap();

        let result = apply_retention_with_result(
            &repo,
            dir.path(),
            Retention::Days(7),
            None,
            &HashSet::new(),
            &HashSet::new(),
        )
        .unwrap();

        assert!(result.deleted.is_empty());
        assert_eq!(result.failed, vec![rec.id.clone()]);
        assert!(repo.get(&rec.id).unwrap().is_some());
        assert_eq!(std::fs::read(raw_audio).unwrap(), b"raw");
        assert_eq!(std::fs::read(processed_audio).unwrap(), b"processed");
        assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".voxely-delete-pending-")
        }));
    }

    #[test]
    fn deleting_record_and_usage_restores_audio_when_sqlite_delete_fails() {
        let dir = tempdir().unwrap();
        let database = dir.path().join("h.db");
        let repo = HistoryRepo::open(&database).unwrap();
        let mut rec = new_recording("provider/model".into());
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        rec.processed_audio_path = Some(format!("{}.processed.wav", rec.id));
        let raw_audio = dir.path().join(rec.raw_audio_path.as_ref().unwrap());
        let processed_audio = dir.path().join(rec.processed_audio_path.as_ref().unwrap());
        std::fs::write(&raw_audio, b"raw audio").unwrap();
        std::fs::write(&processed_audio, b"processed audio").unwrap();
        repo.insert(&rec).unwrap();
        repo.record_attempt(
            &TranscriptionAttempt {
                id: format!("attempt-{}", rec.id),
                recording_id: rec.id.clone(),
                attempt_number: 1,
                started_at: rec.created_at,
                ended_at: Some(rec.created_at),
                outcome: "success".into(),
                error_category: None,
                http_status: Some(200),
                latency_ms: Some(12),
            },
            Some(0.02),
        )
        .unwrap();
        Connection::open(&database)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER reject_recording_delete BEFORE DELETE ON recordings \
                 BEGIN SELECT RAISE(ABORT, 'injected recording delete failure'); END;",
            )
            .unwrap();

        assert!(delete_recording_and_usage(&repo, dir.path(), &rec).is_err());
        assert!(repo.get(&rec.id).unwrap().is_some());
        assert!(raw_audio.is_file());
        assert!(processed_audio.is_file());
        assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".voxely-delete-pending-")
        }));
        let stats = repo
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!((stats.dictations, stats.api_requests), (1, 1));

        Connection::open(&database)
            .unwrap()
            .execute_batch("DROP TRIGGER reject_recording_delete;")
            .unwrap();
        assert!(delete_recording_and_usage(&repo, dir.path(), &rec).unwrap());
        assert!(repo.get(&rec.id).unwrap().is_none());
        assert!(!raw_audio.exists());
        assert!(!processed_audio.exists());
        let stats = repo
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!((stats.dictations, stats.api_requests), (0, 0));
    }

    #[test]
    fn orphan_cleanup_recovers_or_removes_staged_delete_audio() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        let mut rec = new_recording("m".into());
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        repo.insert(&rec).unwrap();
        let staged_for_existing = dir.path().join(format!(
            "{}.voxely-delete-pending-recovery",
            rec.raw_audio_path.as_ref().unwrap()
        ));
        std::fs::write(&staged_for_existing, b"keep this audio").unwrap();

        cleanup_orphans(dir.path(), &repo).unwrap();
        assert_eq!(
            std::fs::read(dir.path().join(rec.raw_audio_path.as_ref().unwrap())).unwrap(),
            b"keep this audio"
        );

        let staged_conflict = dir.path().join(format!(
            "{}.voxely-delete-pending-conflict",
            rec.raw_audio_path.as_ref().unwrap()
        ));
        std::fs::write(&staged_conflict, b"preserve staged copy").unwrap();
        assert!(cleanup_orphans(dir.path(), &repo).is_err());
        assert_eq!(
            std::fs::read(&staged_conflict).unwrap(),
            b"preserve staged copy"
        );
        std::fs::remove_file(&staged_conflict).unwrap();

        repo.delete_with_usage(&rec.id).unwrap();
        let staged_for_deleted = dir.path().join("orphan.wav.voxely-delete-pending-recovery");
        std::fs::write(&staged_for_deleted, b"discard this audio").unwrap();
        cleanup_orphans(dir.path(), &repo).unwrap();
        assert!(!staged_for_deleted.exists());
    }

    #[test]
    fn staged_audio_cleanup_failure_is_reported_after_database_delete() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        let mut rec = new_recording("m".into());
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        let raw_audio = dir.path().join(rec.raw_audio_path.as_ref().unwrap());
        std::fs::write(&raw_audio, b"audio").unwrap();
        repo.insert(&rec).unwrap();

        let result = delete_recording_with_staged_audio(&repo, dir.path(), &rec, |repo, id| {
            let staged_path = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| {
                    path.file_name().is_some_and(|name| {
                        name.to_string_lossy().contains(".voxely-delete-pending-")
                    })
                })
                .unwrap();
            std::fs::remove_file(&staged_path).unwrap();
            std::fs::create_dir(&staged_path).unwrap();
            repo.delete(id)
        });

        assert!(result.is_err());
        assert!(repo.get(&rec.id).unwrap().is_none());
        assert!(!raw_audio.exists());
        assert!(cleanup_orphans(dir.path(), &repo).is_err());
    }

    #[test]
    fn delete_rejects_absolute_and_traversal_audio_paths() {
        let dir = tempdir().unwrap();
        let audio = dir.path().join("audio");
        std::fs::create_dir_all(&audio).unwrap();
        let outside = dir.path().join("outside.wav");
        std::fs::write(&outside, b"must survive").unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();

        for path in [
            "../outside.wav".to_string(),
            outside.to_string_lossy().into_owned(),
        ] {
            let mut rec = new_recording("m".into());
            rec.raw_audio_path = Some(path);
            repo.insert(&rec).unwrap();

            assert!(delete_recording(&repo, &audio, &rec).is_err());
            assert!(repo.get(&rec.id).unwrap().is_some());
            assert_eq!(std::fs::read(&outside).unwrap(), b"must survive");
        }
    }

    #[test]
    fn preview_counts_protected_filter_inventory_without_selecting_it() {
        let dir = tempdir().unwrap();
        let audio = dir.path().join("audio");
        std::fs::create_dir_all(&audio).unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        let protected_file = audio.join("filter-sample.wav");
        std::fs::write(&protected_file, [0u8; 32]).unwrap();
        let mut protected_names = HashSet::new();
        protected_names.insert("filter-sample.wav".into());

        let preview = preview_retention(
            &repo,
            &audio,
            Retention::Forever,
            Some(8),
            &HashSet::new(),
            &protected_names,
        )
        .unwrap();

        assert_eq!(preview.entries_to_delete, 0);
        assert_eq!(preview.files_to_delete, 0);
        assert_eq!(preview.bytes_to_free, 0);
        assert_eq!(preview.protected_bytes, 32);
        assert_eq!(preview.total_audio_bytes, 32);
    }

    #[test]
    fn clear_filter_artifacts_attempts_all_and_reports_only_failures() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("filter-sample.wav"), b"sample").unwrap();
        std::fs::create_dir(dir.path().join("filter-preview.wav")).unwrap();
        std::fs::write(dir.path().join("filter-preview-original.wav"), b"preview").unwrap();

        let failed = remove_filter_artifacts(dir.path());

        assert_eq!(
            failed,
            vec![dir.path().join("filter-preview.wav").to_string_lossy()]
        );
        assert!(!dir.path().join("filter-sample.wav").exists());
        assert!(dir.path().join("filter-preview.wav").is_dir());
        assert!(!dir.path().join("filter-preview-original.wav").exists());
    }
}
