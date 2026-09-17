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
pub struct HistoryPage {
    pub items: Vec<Recording>,
    pub next_cursor: Option<String>,
    pub total: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistorySummaryPage {
    pub items: Vec<RecordingSummary>,
    pub next_cursor: Option<String>,
    pub total: i64,
    pub has_more: bool,
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
            CREATE INDEX IF NOT EXISTS idx_recordings_search ON recordings(model);
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

    pub fn update(&self, rec: &Recording) -> Result<bool, AppError> {
        let changed = self
            .conn
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
        Ok(changed > 0)
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
        self.list_page(None, limit, None).map(|page| page.items)
    }

    pub fn list_all(&self) -> Result<Vec<Recording>, AppError> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM recordings ORDER BY created_at DESC")
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let rows = stmt
            .query_map([], row_to_recording)
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::StorageFailed(e.to_string()))
    }

    pub fn count(&self, query: Option<&str>) -> Result<i64, AppError> {
        match query.map(str::trim).filter(|q| !q.is_empty()) {
            None => self
                .conn
                .query_row("SELECT COUNT(*) FROM recordings", [], |row| row.get(0))
                .map_err(|e| AppError::StorageFailed(e.to_string())),
            Some(q) => {
                let like = format!("%{q}%");
                self.conn
                    .query_row(
                        "SELECT COUNT(*) FROM recordings WHERE transcript LIKE ?1
                         OR last_error_message LIKE ?1 OR last_error_code LIKE ?1 OR model LIKE ?1",
                        params![like],
                        |row| row.get(0),
                    )
                    .map_err(|e| AppError::StorageFailed(e.to_string()))
            }
        }
    }

    pub fn list_page(
        &self,
        cursor: Option<&str>,
        limit: i64,
        query: Option<&str>,
    ) -> Result<HistoryPage, AppError> {
        let limit = limit.clamp(1, 200);
        let search = query.map(str::trim).filter(|q| !q.is_empty());
        let mut items = match (cursor, search) {
            (None, None) => {
                let mut stmt = self
                    .conn
                    .prepare("SELECT * FROM recordings ORDER BY created_at DESC, id DESC LIMIT ?1")
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                let rows = stmt
                    .query_map(params![limit + 1], row_to_recording)
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?
            }
            (Some(id), None) => {
                let Some(anchor) = self.get(id)? else {
                    return Ok(HistoryPage {
                        items: Vec::new(),
                        next_cursor: None,
                        total: self.count(None)?,
                        has_more: false,
                    });
                };
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT * FROM recordings WHERE created_at < ?1
                         OR (created_at = ?1 AND id < ?2)
                         ORDER BY created_at DESC, id DESC LIMIT ?3",
                    )
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                let rows = stmt
                    .query_map(
                        params![anchor.created_at.to_rfc3339(), anchor.id, limit + 1],
                        row_to_recording,
                    )
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?
            }
            (None, Some(q)) => {
                let like = format!("%{q}%");
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT * FROM recordings WHERE transcript LIKE ?1
                         OR last_error_message LIKE ?1 OR last_error_code LIKE ?1 OR model LIKE ?1
                         ORDER BY created_at DESC, id DESC LIMIT ?2",
                    )
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                let rows = stmt
                    .query_map(params![like, limit + 1], row_to_recording)
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?
            }
            (Some(id), Some(q)) => {
                let Some(anchor) = self.get(id)? else {
                    return Ok(HistoryPage {
                        items: Vec::new(),
                        next_cursor: None,
                        total: self.count(Some(q))?,
                        has_more: false,
                    });
                };
                let like = format!("%{q}%");
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT * FROM recordings WHERE (transcript LIKE ?1
                         OR last_error_message LIKE ?1 OR last_error_code LIKE ?1 OR model LIKE ?1)
                         AND (created_at < ?2 OR (created_at = ?2 AND id < ?3))
                         ORDER BY created_at DESC, id DESC LIMIT ?4",
                    )
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                let rows = stmt
                    .query_map(
                        params![like, anchor.created_at.to_rfc3339(), anchor.id, limit + 1],
                        row_to_recording,
                    )
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?
            }
        };
        let has_more = items.len() as i64 > limit;
        if has_more {
            items.truncate(limit as usize);
        }
        let next_cursor = if has_more {
            items.last().map(|rec| rec.id.clone())
        } else {
            None
        };
        Ok(HistoryPage {
            items,
            next_cursor,
            total: self.count(search)?,
            has_more,
        })
    }

    pub fn list_summaries(&self, limit: i64) -> Result<Vec<RecordingSummary>, AppError> {
        let page = self.list_page(None, limit, None)?;
        Ok(page.items.into_iter().map(recording_to_summary).collect())
    }

    pub fn list_summaries_page(
        &self,
        cursor: Option<&str>,
        limit: i64,
        query: Option<&str>,
    ) -> Result<HistorySummaryPage, AppError> {
        let page = self.list_page(cursor, limit, query)?;
        Ok(HistorySummaryPage {
            items: page.items.into_iter().map(recording_to_summary).collect(),
            next_cursor: page.next_cursor,
            total: page.total,
            has_more: page.has_more,
        })
    }

    pub fn delete(&self, id: &str) -> Result<bool, AppError> {
        let changed = self
            .conn
            .execute("DELETE FROM recordings WHERE id=?1", params![id])
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        Ok(changed > 0)
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

    pub fn list_attempts(&self, recording_id: &str) -> Result<Vec<TranscriptionAttempt>, AppError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, recording_id, attempt_number, started_at, ended_at, outcome,
                 error_category, http_status, latency_ms
                 FROM transcription_attempts WHERE recording_id=?1 ORDER BY attempt_number",
            )
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let rows = stmt
            .query_map(params![recording_id], |row| {
                Ok(TranscriptionAttempt {
                    id: row.get("id")?,
                    recording_id: row.get("recording_id")?,
                    attempt_number: row.get("attempt_number")?,
                    started_at: parse_time(row.get::<_, String>("started_at")?),
                    ended_at: row.get::<_, Option<String>>("ended_at")?.map(parse_time),
                    outcome: row.get("outcome")?,
                    error_category: row.get("error_category")?,
                    http_status: row.get("http_status")?,
                    latency_ms: row.get("latency_ms")?,
                })
            })
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::StorageFailed(e.to_string()))
    }

    pub fn total_audio_bytes(&self, root: &Path) -> u64 {
        let mut total = 0u64;
        if let Ok(list) = self.list_all() {
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

fn recording_to_summary(rec: Recording) -> RecordingSummary {
    RecordingSummary {
        id: rec.id,
        created_at: rec.created_at,
        duration_ms: rec.duration_ms,
        raw_audio_path: rec.raw_audio_path,
        processed_audio_path: rec.processed_audio_path,
        transcript: rec.transcript,
        status: rec.status,
        provider: rec.provider,
        model: rec.model,
        attempt_count: rec.attempt_count,
        last_error_code: rec.last_error_code,
        last_error_message: rec.last_error_message,
        cost: rec.cost,
        generation_id: rec.generation_id,
        latency_ms: rec.latency_ms,
    }
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

    #[test]
    fn missing_update_reports_no_row() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        let rec = new_recording("m".into());
        assert!(!repo.update(&rec).unwrap());
    }

    #[test]
    fn search_and_cursor_cover_full_table() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        for i in 0..8 {
            let mut rec = new_recording("openai/gpt-transcribe".into());
            rec.transcript = Some(if i == 7 {
                "needle unique".into()
            } else {
                format!("row {i}")
            });
            rec.created_at -= chrono::Duration::seconds(i);
            rec.updated_at = rec.created_at;
            rec.id = format!("id-{i}");
            repo.insert(&rec).unwrap();
        }
        let page = repo.list_page(None, 3, None).unwrap();
        assert_eq!(page.items.len(), 3);
        assert!(page.has_more);
        let next = repo
            .list_page(page.next_cursor.as_deref(), 3, None)
            .unwrap();
        assert_eq!(next.items.len(), 3);
        let found = repo.list_page(None, 10, Some("needle")).unwrap();
        assert_eq!(found.total, 1);
        assert_eq!(found.items[0].transcript.as_deref(), Some("needle unique"));
    }
}
