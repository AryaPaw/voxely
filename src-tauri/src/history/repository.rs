use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RecordingStatus {
    Processing,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Recording {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub duration_ms: i64,
    pub raw_audio_path: Option<String>,
    pub processed_audio_path: Option<String>,
    pub transcript: Option<String>,
    pub status: RecordingStatus,
    pub provider: String,
    pub model: String,
    pub attempt_count: i64,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub request_started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub usage_json: Option<String>,
    pub cost: Option<f64>,
    pub generation_id: Option<String>,
    pub latency_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSummary {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub duration_ms: i64,
    pub raw_audio_path: Option<String>,
    pub processed_audio_path: Option<String>,
    pub transcript: Option<String>,
    pub status: RecordingStatus,
    pub provider: String,
    pub model: String,
    pub attempt_count: i64,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub cost: Option<f64>,
    pub generation_id: Option<String>,
    pub latency_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionAttempt {
    pub id: String,
    pub recording_id: String,
    pub attempt_number: i64,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub outcome: String,
    pub error_category: Option<String>,
    pub http_status: Option<i64>,
    pub latency_ms: Option<i64>,
}

pub struct HistoryRepo {
    conn: Connection,
}

impl HistoryRepo {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        }
        let conn = Connection::open(path).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode=WAL;
            PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS recordings (
                id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                raw_audio_path TEXT,
                processed_audio_path TEXT,
                transcript TEXT,
                status TEXT NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                attempt_count INTEGER NOT NULL DEFAULT 0,
                last_error_code TEXT,
                last_error_message TEXT,
                request_started_at TEXT,
                completed_at TEXT,
                usage_json TEXT,
                cost REAL,
                generation_id TEXT,
                latency_ms INTEGER
            );
            CREATE TABLE IF NOT EXISTS transcription_attempts (
                id TEXT PRIMARY KEY,
                recording_id TEXT NOT NULL,
                attempt_number INTEGER NOT NULL,
                started_at TEXT NOT NULL,
                ended_at TEXT,
                outcome TEXT NOT NULL,
                error_category TEXT,
                http_status INTEGER,
                latency_ms INTEGER,
                FOREIGN KEY(recording_id) REFERENCES recordings(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_recordings_created ON recordings(created_at);
            "#,
        )
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        Ok(Self { conn })
    }

    pub fn insert(&self, rec: &Recording) -> Result<(), AppError> {
        self.conn
            .execute(
                "INSERT INTO recordings (
                    id, created_at, updated_at, duration_ms, raw_audio_path, processed_audio_path,
                    transcript, status, provider, model, attempt_count, last_error_code,
                    last_error_message, request_started_at, completed_at, usage_json, cost,
                    generation_id, latency_ms
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
                params![
                    rec.id,
                    rec.created_at.to_rfc3339(),
                    rec.updated_at.to_rfc3339(),
                    rec.duration_ms,
                    rec.raw_audio_path,
                    rec.processed_audio_path,
                    rec.transcript,
                    status_str(&rec.status),
                    rec.provider,
                    rec.model,
                    rec.attempt_count,
                    rec.last_error_code,
                    rec.last_error_message,
                    rec.request_started_at.map(|t| t.to_rfc3339()),
                    rec.completed_at.map(|t| t.to_rfc3339()),
                    rec.usage_json,
                    rec.cost,
                    rec.generation_id,
                    rec.latency_ms
                ],
            )
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        Ok(())
    }

    pub fn update(&self, rec: &Recording) -> Result<(), AppError> {
        self.conn
            .execute(
                "UPDATE recordings SET updated_at=?2, duration_ms=?3, raw_audio_path=?4,
                 processed_audio_path=?5, transcript=?6, status=?7, provider=?8, model=?9,
                 attempt_count=?10, last_error_code=?11, last_error_message=?12,
                 request_started_at=?13, completed_at=?14, usage_json=?15, cost=?16,
                 generation_id=?17, latency_ms=?18 WHERE id=?1",
                params![
                    rec.id,
                    rec.updated_at.to_rfc3339(),
                    rec.duration_ms,
                    rec.raw_audio_path,
                    rec.processed_audio_path,
                    rec.transcript,
                    status_str(&rec.status),
                    rec.provider,
                    rec.model,
                    rec.attempt_count,
                    rec.last_error_code,
                    rec.last_error_message,
                    rec.request_started_at.map(|t| t.to_rfc3339()),
                    rec.completed_at.map(|t| t.to_rfc3339()),
                    rec.usage_json,
                    rec.cost,
                    rec.generation_id,
                    rec.latency_ms
                ],
            )
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<Recording>, AppError> {
        self.conn
            .query_row(
                "SELECT * FROM recordings WHERE id=?1",
                params![id],
                row_to_recording,
            )
            .optional()
            .map_err(|e| AppError::StorageFailed(e.to_string()))
    }

    pub fn list(&self, limit: i64) -> Result<Vec<Recording>, AppError> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM recordings ORDER BY created_at DESC LIMIT ?1")
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let rows = stmt
            .query_map(params![limit], row_to_recording)
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::StorageFailed(e.to_string()))
    }

    pub fn list_summaries(&self, limit: i64) -> Result<Vec<RecordingSummary>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, created_at, duration_ms, raw_audio_path, processed_audio_path,
                    transcript, status, provider, model, attempt_count, last_error_code,
                    last_error_message, cost, generation_id, latency_ms
             FROM recordings ORDER BY created_at DESC LIMIT ?1",
            )
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let rows = stmt
            .query_map(params![limit], row_to_summary)
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::StorageFailed(e.to_string()))
    }

    pub fn delete(&self, id: &str) -> Result<(), AppError> {
        self.conn
            .execute("DELETE FROM recordings WHERE id=?1", params![id])
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        Ok(())
    }

    pub fn delete_all(&self) -> Result<(), AppError> {
        self.conn
            .execute("DELETE FROM recordings", [])
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        Ok(())
    }

    pub fn insert_attempt(&self, attempt: &TranscriptionAttempt) -> Result<(), AppError> {
        self.conn
            .execute(
                "INSERT INTO transcription_attempts (
                    id, recording_id, attempt_number, started_at, ended_at, outcome,
                    error_category, http_status, latency_ms
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    attempt.id,
                    attempt.recording_id,
                    attempt.attempt_number,
                    attempt.started_at.to_rfc3339(),
                    attempt.ended_at.map(|t| t.to_rfc3339()),
                    attempt.outcome,
                    attempt.error_category,
                    attempt.http_status,
                    attempt.latency_ms
                ],
            )
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        Ok(())
    }

    pub fn older_than(&self, cutoff: DateTime<Utc>) -> Result<Vec<Recording>, AppError> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM recordings WHERE created_at < ?1")
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let rows = stmt
            .query_map(params![cutoff.to_rfc3339()], row_to_recording)
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::StorageFailed(e.to_string()))
    }

    pub fn total_audio_bytes(&self, root: &Path) -> u64 {
        let mut total = 0u64;
        if let Ok(list) = self.list(10_000) {
            for rec in list {
                for path in [rec.raw_audio_path, rec.processed_audio_path]
                    .into_iter()
                    .flatten()
                {
                    let full = root.join(path);
                    if let Ok(meta) = std::fs::metadata(full) {
                        total += meta.len();
                    }
                }
            }
        }
        total
    }
}

fn status_str(status: &RecordingStatus) -> &'static str {
    match status {
        RecordingStatus::Processing => "processing",
        RecordingStatus::Completed => "completed",
        RecordingStatus::Failed => "failed",
        RecordingStatus::Interrupted => "interrupted",
    }
}

fn parse_status(value: String) -> RecordingStatus {
    match value.as_str() {
        "completed" => RecordingStatus::Completed,
        "failed" => RecordingStatus::Failed,
        "interrupted" => RecordingStatus::Interrupted,
        _ => RecordingStatus::Processing,
    }
}

fn row_to_recording(row: &rusqlite::Row<'_>) -> rusqlite::Result<Recording> {
    Ok(Recording {
        id: row.get("id")?,
        created_at: parse_time(row.get::<_, String>("created_at")?),
        updated_at: parse_time(row.get::<_, String>("updated_at")?),
        duration_ms: row.get("duration_ms")?,
        raw_audio_path: row.get("raw_audio_path")?,
        processed_audio_path: row.get("processed_audio_path")?,
        transcript: row.get("transcript")?,
        status: parse_status(row.get("status")?),
        provider: row.get("provider")?,
        model: row.get("model")?,
        attempt_count: row.get("attempt_count")?,
        last_error_code: row.get("last_error_code")?,
        last_error_message: row.get("last_error_message")?,
        request_started_at: row
            .get::<_, Option<String>>("request_started_at")?
            .map(parse_time),
        completed_at: row
            .get::<_, Option<String>>("completed_at")?
            .map(parse_time),
        usage_json: row.get("usage_json")?,
        cost: row.get("cost")?,
        generation_id: row.get("generation_id")?,
        latency_ms: row.get("latency_ms")?,
    })
}

fn row_to_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecordingSummary> {
    Ok(RecordingSummary {
        id: row.get("id")?,
        created_at: parse_time(row.get::<_, String>("created_at")?),
        duration_ms: row.get("duration_ms")?,
        raw_audio_path: row.get("raw_audio_path")?,
        processed_audio_path: row.get("processed_audio_path")?,
        transcript: row.get("transcript")?,
        status: parse_status(row.get("status")?),
        provider: row.get("provider")?,
        model: row.get("model")?,
        attempt_count: row.get("attempt_count")?,
        last_error_code: row.get("last_error_code")?,
        last_error_message: row.get("last_error_message")?,
        cost: row.get("cost")?,
        generation_id: row.get("generation_id")?,
        latency_ms: row.get("latency_ms")?,
    })
}

fn parse_time(value: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&value)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

pub fn new_recording(model: String) -> Recording {
    let now = Utc::now();
    Recording {
        id: Uuid::new_v4().to_string(),
        created_at: now,
        updated_at: now,
        duration_ms: 0,
        raw_audio_path: None,
        processed_audio_path: None,
        transcript: None,
        status: RecordingStatus::Processing,
        provider: "openrouter".into(),
        model,
        attempt_count: 0,
        last_error_code: None,
        last_error_message: None,
        request_started_at: None,
        completed_at: None,
        usage_json: None,
        cost: None,
        generation_id: None,
        latency_ms: None,
    }
}

pub fn audio_dir(root: &Path) -> PathBuf {
    root.join("audio")
}

pub fn can_retry_from_history(rec: &Recording) -> bool {
    rec.raw_audio_path.is_some() || rec.processed_audio_path.is_some()
}

pub fn history_listen_name(rec: &Recording) -> Option<&String> {
    rec.processed_audio_path
        .as_ref()
        .or(rec.raw_audio_path.as_ref())
}

pub fn history_play_name(rec: &Recording, keep_original: bool) -> Option<&String> {
    if rec.processed_audio_path.is_some() {
        return rec.processed_audio_path.as_ref();
    }
    if keep_original {
        rec.raw_audio_path.as_ref()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn insert_and_list() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        let rec = new_recording("openai/gpt-transcribe".into());
        repo.insert(&rec).unwrap();
        assert_eq!(repo.list(10).unwrap().len(), 1);
        repo.delete(&rec.id).unwrap();
        assert!(repo.list(10).unwrap().is_empty());
    }

    #[test]
    fn list_summaries_omits_usage_json() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.transcript = Some("hello from search".into());
        rec.usage_json = Some("{\"tokens\":1}".into());
        repo.insert(&rec).unwrap();
        let summaries = repo.list_summaries(10).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(
            summaries[0].transcript.as_deref(),
            Some("hello from search")
        );
        let encoded = serde_json::to_string(&summaries[0]).unwrap();
        assert!(!encoded.contains("usageJson"));
        assert!(!encoded.contains("updatedAt"));
    }

    #[test]
    fn completed_recordings_with_audio_can_retry() {
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Completed;
        rec.transcript = Some("old".into());
        rec.processed_audio_path = Some("1.processed.wav".into());
        assert!(can_retry_from_history(&rec));
        rec.processed_audio_path = None;
        rec.raw_audio_path = None;
        assert!(!can_retry_from_history(&rec));
        rec.processed_audio_path = Some("1.processed.wav".into());
        rec.raw_audio_path = Some("1.wav".into());
        assert_eq!(
            history_listen_name(&rec).map(String::as_str),
            Some("1.processed.wav")
        );
        rec.processed_audio_path = None;
        assert_eq!(history_listen_name(&rec).map(String::as_str), Some("1.wav"));
        assert!(history_play_name(&rec, false).is_none());
        assert_eq!(
            history_play_name(&rec, true).map(String::as_str),
            Some("1.wav")
        );
    }
}
