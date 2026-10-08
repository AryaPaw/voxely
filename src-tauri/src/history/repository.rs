use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageStatistics {
    pub dictations: i64,
    pub completed: i64,
    pub failed: i64,
    pub interrupted: i64,
    pub api_requests: i64,
    pub reported_cost_usd: f64,
    pub unpriced_attempts: i64,
    pub audio_duration_ms: i64,
    pub daily: Vec<UsageStatisticsBucket>,
    pub models: Vec<UsageStatisticsModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageStatisticsBucket {
    pub date: String,
    pub dictations: i64,
    pub api_requests: i64,
    pub reported_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageStatisticsModel {
    pub model: String,
    pub dictations: i64,
    pub api_requests: i64,
    pub reported_cost_usd: f64,
    pub unpriced_attempts: i64,
}

impl HistoryRepo {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        }
        let conn = Connection::open(path).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
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
                latency_ms INTEGER,
                search_text TEXT NOT NULL DEFAULT ''
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
            CREATE INDEX IF NOT EXISTS idx_attempts_recording ON transcription_attempts(recording_id);
            CREATE TABLE IF NOT EXISTS usage_dictations (
                recording_id TEXT PRIMARY KEY, created_at TEXT NOT NULL, model TEXT NOT NULL,
                duration_ms INTEGER NOT NULL, status TEXT NOT NULL, cost_usd REAL
            );
            CREATE TABLE IF NOT EXISTS usage_attempts (
                attempt_id TEXT PRIMARY KEY, recording_id TEXT NOT NULL, occurred_at TEXT NOT NULL,
                model TEXT NOT NULL, cost_usd REAL
            );
            CREATE INDEX IF NOT EXISTS idx_usage_dictations_time ON usage_dictations(created_at);
            CREATE INDEX IF NOT EXISTS idx_usage_attempts_time ON usage_attempts(occurred_at);
            CREATE TABLE IF NOT EXISTS usage_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS usage_suppressed (recording_id TEXT PRIMARY KEY);
            "#,
        )
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        ensure_search_text_column(&conn)?;
        let repo = Self { conn };
        repo.backfill_search_text()?;
        repo.backfill_usage_statistics()?;
        repo.remove_legacy_cancelled_usage()?;
        repo.prune_usage_statistics(Utc::now() - chrono::Duration::days(90))?;
        Ok(repo)
    }

    pub fn insert(&self, rec: &Recording) -> Result<(), AppError> {
        let tx = self.conn.unchecked_transaction().map_err(storage_err)?;
        tx.execute(
            "INSERT INTO recordings (
                    id, created_at, updated_at, duration_ms, raw_audio_path, processed_audio_path,
                    transcript, status, provider, model, attempt_count, last_error_code,
                    last_error_message, request_started_at, completed_at, usage_json, cost,
                    generation_id, latency_ms, search_text
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
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
                rec.latency_ms,
                recording_search_text(rec)
            ],
        )
        .map_err(storage_err)?;
        tx.execute(
            "DELETE FROM usage_suppressed WHERE recording_id=?1",
            params![rec.id],
        )
        .map_err(storage_err)?;
        tx.execute("INSERT OR IGNORE INTO usage_dictations(recording_id,created_at,model,duration_ms,status,cost_usd) VALUES(?1,?2,?3,?4,?5,NULL)", params![rec.id, rec.created_at.to_rfc3339(), rec.model, rec.duration_ms, status_str(&rec.status)]).map_err(storage_err)?;
        tx.commit().map_err(storage_err)?;
        Ok(())
    }

    fn backfill_usage_statistics(&self) -> Result<(), AppError> {
        self.conn
            .execute_batch("BEGIN IMMEDIATE;")
            .map_err(storage_err)?;
        let result = (|| -> Result<(), AppError> {
            let migrated: bool = self
                .conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM usage_meta WHERE key='backfill-v1')",
                    [],
                    |r| r.get(0),
                )
                .map_err(storage_err)?;
            if migrated {
                self.conn.execute_batch("COMMIT;").map_err(storage_err)?;
                return Ok(());
            }
            self.conn.execute("INSERT OR IGNORE INTO usage_dictations(recording_id,created_at,model,duration_ms,status,cost_usd) SELECT id,created_at,model,duration_ms,status,cost FROM recordings", []).map_err(storage_err)?;
            self.conn.execute("INSERT OR IGNORE INTO usage_attempts(attempt_id,recording_id,occurred_at,model,cost_usd) SELECT a.id,a.recording_id,COALESCE(a.ended_at,a.started_at),'unknown',NULL FROM transcription_attempts a JOIN recordings r ON r.id=a.recording_id", []).map_err(storage_err)?;
            let legacy_rows = {
                let mut stmt = self
                    .conn
                    .prepare("SELECT id,created_at,attempt_count FROM recordings")
                    .map_err(storage_err)?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, i64>(2)?,
                        ))
                    })
                    .map_err(storage_err)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(storage_err)?
            };
            for (recording_id, created_at, attempts) in legacy_rows {
                let existing: i64 = self
                    .conn
                    .query_row(
                        "SELECT COUNT(*) FROM transcription_attempts WHERE recording_id=?1",
                        params![recording_id],
                        |r| r.get(0),
                    )
                    .map_err(storage_err)?;
                for number in (existing + 1)..=attempts.max(existing) {
                    let id = format!("legacy:{recording_id}:{number}");
                    self.conn.execute("INSERT OR IGNORE INTO usage_attempts(attempt_id,recording_id,occurred_at,model,cost_usd) VALUES(?1,?2,?3,'unknown',NULL)", params![id, recording_id, created_at]).map_err(storage_err)?;
                }
            }
            self.conn.execute("UPDATE usage_attempts SET cost_usd=(SELECT r.cost FROM recordings r WHERE r.id=usage_attempts.recording_id) WHERE cost_usd IS NULL AND attempt_id=(SELECT a.id FROM transcription_attempts a WHERE a.recording_id=usage_attempts.recording_id AND a.outcome='success' ORDER BY COALESCE(a.ended_at,a.started_at) DESC LIMIT 1)", []).map_err(storage_err)?;
            self.conn.execute("UPDATE usage_dictations SET cost_usd=NULL WHERE EXISTS(SELECT 1 FROM usage_attempts a WHERE a.recording_id=usage_dictations.recording_id AND a.cost_usd IS NOT NULL)", []).map_err(storage_err)?;
            self.conn
                .execute(
                    "INSERT INTO usage_meta(key,value) VALUES('backfill-v1','done')",
                    [],
                )
                .map_err(storage_err)?;
            self.conn.execute_batch("COMMIT;").map_err(storage_err)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = self.conn.execute_batch("ROLLBACK;");
        }
        result
    }

    fn backfill_search_text(&self) -> Result<(), AppError> {
        self.conn
            .execute_batch("BEGIN IMMEDIATE;")
            .map_err(storage_err)?;
        let result = (|| -> Result<(), AppError> {
            let migrated: bool = self
                .conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM usage_meta WHERE key='search-fold-v2')",
                    [],
                    |row| row.get(0),
                )
                .map_err(storage_err)?;
            if migrated {
                self.conn.execute_batch("COMMIT;").map_err(storage_err)?;
                return Ok(());
            }
            let records = {
                let mut stmt = self
                    .conn
                    .prepare("SELECT id,transcript,last_error_message,last_error_code,model FROM recordings")
                    .map_err(storage_err)?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    })
                    .map_err(storage_err)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(storage_err)?
            };
            for (id, transcript, error_message, error_code, model) in records {
                let search_text = normalize_search_text(&[
                    transcript.as_deref().unwrap_or_default(),
                    error_message.as_deref().unwrap_or_default(),
                    error_code.as_deref().unwrap_or_default(),
                    &model,
                ]);
                self.conn
                    .execute(
                        "UPDATE recordings SET search_text=?1 WHERE id=?2",
                        params![search_text, id],
                    )
                    .map_err(storage_err)?;
            }
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO usage_meta(key,value) VALUES('search-fold-v2','done')",
                    [],
                )
                .map_err(storage_err)?;
            self.conn.execute_batch("COMMIT;").map_err(storage_err)
        })();
        if result.is_err() {
            let _ = self.conn.execute_batch("ROLLBACK;");
        }
        result
    }

    fn remove_legacy_cancelled_usage(&self) -> Result<(), AppError> {
        self.conn
            .execute_batch("BEGIN IMMEDIATE;")
            .map_err(storage_err)?;
        let result = (|| -> Result<(), AppError> {
            let migrated: bool = self
                .conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM usage_meta WHERE key='remove-cancelled-v2')",
                    [],
                    |row| row.get(0),
                )
                .map_err(storage_err)?;
            if !migrated {
                // A cancelled retry in older builds could overwrite a previously completed
                // dictation's status while retaining its success payload. Restore those rows
                // before tombstoning genuinely cancelled live recordings.
                let recoverable_successes = {
                    let mut stmt = self
                        .conn
                        .prepare(
                            "SELECT * FROM recordings WHERE last_error_code='Cancelled' \
                             AND completed_at IS NOT NULL",
                        )
                        .map_err(storage_err)?;
                    let rows = stmt.query_map([], row_to_recording).map_err(storage_err)?;
                    rows.collect::<Result<Vec<_>, _>>().map_err(storage_err)?
                };
                for mut rec in recoverable_successes {
                    rec.status = RecordingStatus::Completed;
                    rec.last_error_code = None;
                    rec.last_error_message = None;
                    rec.updated_at = Utc::now();
                    Self::update_in_transaction(&self.conn, &rec)?;
                }

                // Preserve a minimal, hidden tombstone until the row can be physically
                // removed. Usage and transcript payloads are cleared transactionally.
                self.conn.execute("UPDATE recordings SET updated_at=?1,duration_ms=0,raw_audio_path=NULL,processed_audio_path=NULL,transcript=NULL,status='failed',provider='',model='',attempt_count=0,last_error_message='Cancelled',request_started_at=NULL,completed_at=NULL,usage_json=NULL,cost=NULL,generation_id=NULL,latency_ms=NULL,search_text='' WHERE last_error_code='Cancelled'", params![Utc::now().to_rfc3339()]).map_err(storage_err)?;
                self.conn.execute("INSERT OR IGNORE INTO usage_suppressed(recording_id) SELECT id FROM recordings WHERE last_error_code='Cancelled'", []).map_err(storage_err)?;
                self.conn.execute("DELETE FROM usage_attempts WHERE recording_id IN (SELECT id FROM recordings WHERE last_error_code='Cancelled') OR recording_id IN (SELECT u.recording_id FROM usage_dictations u WHERE u.status='interrupted' AND NOT EXISTS (SELECT 1 FROM recordings r WHERE r.id=u.recording_id) AND NOT EXISTS (SELECT 1 FROM usage_attempts a WHERE a.recording_id=u.recording_id))", []).map_err(storage_err)?;
                self.conn.execute("DELETE FROM usage_dictations WHERE recording_id IN (SELECT id FROM recordings WHERE last_error_code='Cancelled') OR (status='interrupted' AND NOT EXISTS (SELECT 1 FROM recordings r WHERE r.id=usage_dictations.recording_id) AND NOT EXISTS (SELECT 1 FROM usage_attempts a WHERE a.recording_id=usage_dictations.recording_id))", []).map_err(storage_err)?;
                self.conn.execute("DELETE FROM transcription_attempts WHERE recording_id IN (SELECT id FROM recordings WHERE last_error_code='Cancelled')", []).map_err(storage_err)?;
                self.conn
                    .execute(
                        "INSERT INTO usage_meta(key,value) VALUES('remove-cancelled-v2','done')",
                        [],
                    )
                    .map_err(storage_err)?;
            }
            self.conn.execute_batch("COMMIT;").map_err(storage_err)
        })();
        if result.is_err() {
            let _ = self.conn.execute_batch("ROLLBACK;");
            return result;
        }
        // Keep sanitized cancellation tombstones until startup audio cleanup confirms that
        // deterministic raw, processed, STT, and staged artifacts are gone.
        let _ = self.conn.execute(
            "DELETE FROM usage_suppressed WHERE recording_id NOT IN (SELECT id FROM recordings)",
            [],
        );
        Ok(())
    }

    pub fn prune_usage_statistics(&self, cutoff: DateTime<Utc>) -> Result<(), AppError> {
        self.conn
            .execute(
                "DELETE FROM usage_dictations WHERE created_at < ?1",
                params![cutoff.to_rfc3339()],
            )
            .map_err(storage_err)?;
        self.conn
            .execute(
                "DELETE FROM usage_attempts WHERE occurred_at < ?1",
                params![cutoff.to_rfc3339()],
            )
            .map_err(storage_err)?;
        self.conn
            .execute(
                "DELETE FROM usage_suppressed WHERE recording_id NOT IN (SELECT id FROM recordings)",
                [],
            )
            .map_err(storage_err)?;
        Ok(())
    }

    pub fn get_usage_statistics(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<UsageStatistics, AppError> {
        self.prune_usage_statistics(Utc::now() - chrono::Duration::days(90))?;
        let start_s = start.to_rfc3339();
        let end_s = end.to_rfc3339();
        let mut stats = UsageStatistics {
            dictations: 0,
            completed: 0,
            failed: 0,
            interrupted: 0,
            api_requests: 0,
            reported_cost_usd: 0.0,
            unpriced_attempts: 0,
            audio_duration_ms: 0,
            daily: Vec::new(),
            models: Vec::new(),
        };
        let mut daily = std::collections::BTreeMap::<String, UsageStatisticsBucket>::new();
        let mut models = std::collections::BTreeMap::<String, UsageStatisticsModel>::new();
        let mut stmt = self.conn.prepare("SELECT created_at,model,duration_ms,status,cost_usd FROM usage_dictations WHERE created_at>=?1 AND created_at<?2").map_err(storage_err)?;
        let rows = stmt
            .query_map(params![start_s, end_s], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<f64>>(4)?,
                ))
            })
            .map_err(storage_err)?;
        for row in rows {
            let (at, model, duration, status, cost) = row.map_err(storage_err)?;
            let date = local_date(&at);
            stats.dictations += 1;
            stats.audio_duration_ms += duration.max(0);
            match status.as_str() {
                "completed" => stats.completed += 1,
                "failed" => stats.failed += 1,
                "interrupted" => stats.interrupted += 1,
                _ => {}
            }
            let d = daily
                .entry(date.clone())
                .or_insert_with(|| UsageStatisticsBucket {
                    date,
                    dictations: 0,
                    api_requests: 0,
                    reported_cost_usd: 0.0,
                });
            d.dictations += 1;
            let m = models
                .entry(model.clone())
                .or_insert_with(|| UsageStatisticsModel {
                    model,
                    dictations: 0,
                    api_requests: 0,
                    reported_cost_usd: 0.0,
                    unpriced_attempts: 0,
                });
            m.dictations += 1;
            if let Some(v) = cost {
                stats.reported_cost_usd += v;
                d.reported_cost_usd += v;
                m.reported_cost_usd += v;
            }
        }
        let mut stmt = self.conn.prepare("SELECT occurred_at,model,cost_usd FROM usage_attempts WHERE occurred_at>=?1 AND occurred_at<?2").map_err(storage_err)?;
        let rows = stmt
            .query_map(params![start_s, end_s], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<f64>>(2)?,
                ))
            })
            .map_err(storage_err)?;
        for row in rows {
            let (at, model, cost) = row.map_err(storage_err)?;
            stats.api_requests += 1;
            let date = local_date(&at);
            let d = daily
                .entry(date.clone())
                .or_insert_with(|| UsageStatisticsBucket {
                    date,
                    dictations: 0,
                    api_requests: 0,
                    reported_cost_usd: 0.0,
                });
            d.api_requests += 1;
            let m = models
                .entry(model.clone())
                .or_insert_with(|| UsageStatisticsModel {
                    model,
                    dictations: 0,
                    api_requests: 0,
                    reported_cost_usd: 0.0,
                    unpriced_attempts: 0,
                });
            m.api_requests += 1;
            if let Some(v) = cost {
                stats.reported_cost_usd += v;
                d.reported_cost_usd += v;
                m.reported_cost_usd += v;
            } else {
                stats.unpriced_attempts += 1;
                m.unpriced_attempts += 1;
            }
        }
        let mut local_day = start.with_timezone(&chrono::Local).date_naive();
        let last_local_day = (end - chrono::Duration::nanoseconds(1))
            .with_timezone(&chrono::Local)
            .date_naive();
        while local_day <= last_local_day {
            let date = local_day.format("%Y-%m-%d").to_string();
            daily.entry(date.clone()).or_insert(UsageStatisticsBucket {
                date,
                dictations: 0,
                api_requests: 0,
                reported_cost_usd: 0.0,
            });
            local_day += chrono::Duration::days(1);
        }
        stats.daily = daily.into_values().collect();
        stats.models = models.into_values().collect();
        Ok(stats)
    }

    pub fn clear_usage_statistics(&self) -> Result<(), AppError> {
        self.conn
            .execute_batch("BEGIN IMMEDIATE;")
            .map_err(storage_err)?;
        let result = (|| -> Result<(), AppError> {
            self.conn.execute("INSERT OR IGNORE INTO usage_suppressed(recording_id) SELECT id FROM recordings", []).map_err(storage_err)?;
            self.conn
                .execute_batch("DELETE FROM usage_dictations; DELETE FROM usage_attempts; COMMIT;")
                .map_err(storage_err)
        })();
        if result.is_err() {
            let _ = self.conn.execute_batch("ROLLBACK;");
        }
        result
    }
    pub fn delete_usage_for_recording(&self, id: &str) -> Result<(), AppError> {
        let tx = self.conn.unchecked_transaction().map_err(storage_err)?;
        tx.execute(
            "DELETE FROM usage_dictations WHERE recording_id=?1",
            params![id],
        )
        .map_err(storage_err)?;
        tx.execute(
            "DELETE FROM usage_attempts WHERE recording_id=?1",
            params![id],
        )
        .map_err(storage_err)?;
        tx.execute(
            "DELETE FROM usage_suppressed WHERE recording_id=?1",
            params![id],
        )
        .map_err(storage_err)?;
        tx.commit().map_err(storage_err)
    }

    pub fn discard_cancelled_recording(&self, id: &str) -> Result<Option<Recording>, AppError> {
        let rec = self.get(id)?;
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|err| AppError::StorageFailed(err.to_string()))?;
        tx.execute(
            "DELETE FROM transcription_attempts WHERE recording_id=?1",
            params![id],
        )
        .map_err(storage_err)?;
        tx.execute(
            "DELETE FROM usage_attempts WHERE recording_id=?1",
            params![id],
        )
        .map_err(storage_err)?;
        tx.execute(
            "DELETE FROM usage_dictations WHERE recording_id=?1",
            params![id],
        )
        .map_err(storage_err)?;
        tx.execute(
            "DELETE FROM usage_suppressed WHERE recording_id=?1",
            params![id],
        )
        .map_err(storage_err)?;
        tx.execute("DELETE FROM recordings WHERE id=?1", params![id])
            .map_err(storage_err)?;
        tx.commit()
            .map_err(|err| AppError::StorageFailed(err.to_string()))?;
        Ok(rec)
    }

    pub fn suppress_cancelled_recording(&self, id: &str) -> Result<bool, AppError> {
        let tx = self.conn.unchecked_transaction().map_err(storage_err)?;
        let changed = tx
            .execute(
                "UPDATE recordings SET updated_at=?2, duration_ms=0, raw_audio_path=NULL,
                 processed_audio_path=NULL, transcript=NULL, status='failed', provider='', model='',
                 attempt_count=0, last_error_code='Cancelled', last_error_message='Cancelled',
                 request_started_at=NULL, completed_at=NULL, usage_json=NULL, cost=NULL,
                 generation_id=NULL, latency_ms=NULL, search_text='' WHERE id=?1",
                params![id, Utc::now().to_rfc3339()],
            )
            .map_err(storage_err)?;
        tx.execute(
            "INSERT OR IGNORE INTO usage_suppressed(recording_id) VALUES(?1)",
            params![id],
        )
        .map_err(storage_err)?;
        tx.execute(
            "DELETE FROM transcription_attempts WHERE recording_id=?1",
            params![id],
        )
        .map_err(storage_err)?;
        tx.execute(
            "DELETE FROM usage_attempts WHERE recording_id=?1",
            params![id],
        )
        .map_err(storage_err)?;
        tx.execute(
            "DELETE FROM usage_dictations WHERE recording_id=?1",
            params![id],
        )
        .map_err(storage_err)?;
        tx.commit().map_err(storage_err)?;
        Ok(changed > 0)
    }

    pub fn update(&self, rec: &Recording) -> Result<bool, AppError> {
        let tx = self.conn.unchecked_transaction().map_err(storage_err)?;
        let changed = Self::update_in_transaction(&tx, rec)?;
        tx.commit().map_err(storage_err)?;
        Ok(changed)
    }

    fn update_in_transaction(conn: &Connection, rec: &Recording) -> Result<bool, AppError> {
        let changed = conn
            .execute(
                "UPDATE recordings SET updated_at=?2, duration_ms=?3, raw_audio_path=?4,
                 processed_audio_path=?5, transcript=?6, status=?7, provider=?8, model=?9,
                 attempt_count=?10, last_error_code=?11, last_error_message=?12,
                 request_started_at=?13, completed_at=?14, usage_json=?15, cost=?16,
                 generation_id=?17, latency_ms=?18, search_text=?19 WHERE id=?1",
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
                    rec.latency_ms,
                    recording_search_text(rec)
                ],
            )
            .map_err(storage_err)?;
        if changed > 0 {
            conn.execute(
                "UPDATE usage_dictations SET model=CASE WHEN cost_usd IS NOT NULL THEN model ELSE ?2 END,
                 duration_ms=?3,status=?4 WHERE recording_id=?1",
                params![
                    rec.id,
                    rec.model,
                    rec.duration_ms,
                    status_str(&rec.status)
                ],
            )
            .map_err(storage_err)?;
        }
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
                .query_row(
                    "SELECT COUNT(*) FROM recordings WHERE COALESCE(last_error_code,'') <> 'Cancelled'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| AppError::StorageFailed(e.to_string())),
            Some(q) => {
                let like = like_pattern(&normalize_search_text(&[q]));
                self.conn
                    .query_row(
                        "SELECT COUNT(*) FROM recordings WHERE COALESCE(last_error_code,'') <> 'Cancelled' AND search_text LIKE ?1 ESCAPE '\\'",
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
                    .prepare("SELECT * FROM recordings WHERE COALESCE(last_error_code,'') <> 'Cancelled' ORDER BY created_at DESC, id DESC LIMIT ?1")
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                let rows = stmt
                    .query_map(params![limit + 1], row_to_recording)
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?
            }
            (Some(cursor), None) => {
                let Some((created_at, id)) = decode_cursor(self, cursor)? else {
                    return self.list_page(None, limit, None);
                };
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT * FROM recordings WHERE COALESCE(last_error_code,'') <> 'Cancelled' AND (created_at < ?1
                         OR (created_at = ?1 AND id < ?2))
                         ORDER BY created_at DESC, id DESC LIMIT ?3",
                    )
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                let rows = stmt
                    .query_map(params![created_at, id, limit + 1], row_to_recording)
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?
            }
            (None, Some(q)) => {
                let like = like_pattern(&normalize_search_text(&[q]));
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT * FROM recordings WHERE COALESCE(last_error_code,'') <> 'Cancelled' AND search_text LIKE ?1 ESCAPE '\\'
                         ORDER BY created_at DESC, id DESC LIMIT ?2",
                    )
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                let rows = stmt
                    .query_map(params![like, limit + 1], row_to_recording)
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?
            }
            (Some(cursor), Some(q)) => {
                let like = like_pattern(&normalize_search_text(&[q]));
                let Some((created_at, id)) = decode_cursor(self, cursor)? else {
                    return self.list_page(None, limit, Some(q));
                };
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT * FROM recordings WHERE COALESCE(last_error_code,'') <> 'Cancelled' AND search_text LIKE ?1 ESCAPE '\\'
                         AND (created_at < ?2 OR (created_at = ?2 AND id < ?3))
                         ORDER BY created_at DESC, id DESC LIMIT ?4",
                    )
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                let rows = stmt
                    .query_map(params![like, created_at, id, limit + 1], row_to_recording)
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
            items.last().map(encode_cursor)
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
        self.list_summaries_page(None, limit, None)
            .map(|page| page.items)
    }

    pub fn list_summaries_page(
        &self,
        cursor: Option<&str>,
        limit: i64,
        query: Option<&str>,
    ) -> Result<HistorySummaryPage, AppError> {
        let limit = limit.clamp(1, 200);
        let search = query.map(str::trim).filter(|q| !q.is_empty());
        let mut items = match (cursor, search) {
            (None, None) => {
                let mut stmt = self
                    .conn
                    .prepare("SELECT id, created_at, duration_ms, raw_audio_path, processed_audio_path, transcript, status, provider, model, attempt_count, last_error_code, last_error_message, cost, generation_id, latency_ms FROM recordings WHERE COALESCE(last_error_code,'') <> 'Cancelled' ORDER BY created_at DESC, id DESC LIMIT ?1")
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                let rows = stmt
                    .query_map(params![limit + 1], row_to_summary)
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?
            }
            (Some(cursor), None) => {
                let Some((created_at, id)) = decode_cursor(self, cursor)? else {
                    return self.list_summaries_page(None, limit, None);
                };
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT id, created_at, duration_ms, raw_audio_path, processed_audio_path, transcript, status, provider, model, attempt_count, last_error_code, last_error_message, cost, generation_id, latency_ms FROM recordings WHERE COALESCE(last_error_code,'') <> 'Cancelled' AND (created_at < ?1
                         OR (created_at = ?1 AND id < ?2))
                         ORDER BY created_at DESC, id DESC LIMIT ?3",
                    )
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                let rows = stmt
                    .query_map(params![created_at, id, limit + 1], row_to_summary)
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?
            }
            (None, Some(q)) => {
                let like = like_pattern(&normalize_search_text(&[q]));
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT id, created_at, duration_ms, raw_audio_path, processed_audio_path, transcript, status, provider, model, attempt_count, last_error_code, last_error_message, cost, generation_id, latency_ms FROM recordings WHERE COALESCE(last_error_code,'') <> 'Cancelled' AND search_text LIKE ?1 ESCAPE '\\'
                         ORDER BY created_at DESC, id DESC LIMIT ?2",
                    )
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                let rows = stmt
                    .query_map(params![like, limit + 1], row_to_summary)
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?
            }
            (Some(cursor), Some(q)) => {
                let like = like_pattern(&normalize_search_text(&[q]));
                let Some((created_at, id)) = decode_cursor(self, cursor)? else {
                    return self.list_summaries_page(None, limit, Some(q));
                };
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT id, created_at, duration_ms, raw_audio_path, processed_audio_path, transcript, status, provider, model, attempt_count, last_error_code, last_error_message, cost, generation_id, latency_ms FROM recordings WHERE COALESCE(last_error_code,'') <> 'Cancelled' AND search_text LIKE ?1 ESCAPE '\\'
                         AND (created_at < ?2 OR (created_at = ?2 AND id < ?3))
                         ORDER BY created_at DESC, id DESC LIMIT ?4",
                    )
                    .map_err(|e| AppError::StorageFailed(e.to_string()))?;
                let rows = stmt
                    .query_map(params![like, created_at, id, limit + 1], row_to_summary)
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
            items
                .last()
                .map(|rec| format!("{}|{}", rec.created_at.to_rfc3339(), rec.id))
        } else {
            None
        };
        Ok(HistorySummaryPage {
            items,
            next_cursor,
            total: self.count(search)?,
            has_more,
        })
    }

    pub fn delete(&self, id: &str) -> Result<bool, AppError> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        tx.execute(
            "DELETE FROM transcription_attempts WHERE recording_id=?1",
            params![id],
        )
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        tx.execute(
            "DELETE FROM usage_suppressed WHERE recording_id=?1",
            params![id],
        )
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let changed = tx
            .execute("DELETE FROM recordings WHERE id=?1", params![id])
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        tx.commit()
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        Ok(changed > 0)
    }

    pub fn delete_with_usage(&self, id: &str) -> Result<bool, AppError> {
        let tx = self.conn.unchecked_transaction().map_err(storage_err)?;
        tx.execute(
            "DELETE FROM transcription_attempts WHERE recording_id=?1",
            params![id],
        )
        .map_err(storage_err)?;
        tx.execute(
            "DELETE FROM usage_attempts WHERE recording_id=?1",
            params![id],
        )
        .map_err(storage_err)?;
        tx.execute(
            "DELETE FROM usage_dictations WHERE recording_id=?1",
            params![id],
        )
        .map_err(storage_err)?;
        tx.execute(
            "DELETE FROM usage_suppressed WHERE recording_id=?1",
            params![id],
        )
        .map_err(storage_err)?;
        let changed = tx
            .execute("DELETE FROM recordings WHERE id=?1", params![id])
            .map_err(storage_err)?;
        tx.commit().map_err(storage_err)?;
        Ok(changed > 0)
    }

    pub fn delete_all(&self) -> Result<(), AppError> {
        self.conn
            .execute("DELETE FROM recordings", [])
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        Ok(())
    }

    pub fn insert_attempt(&self, attempt: &TranscriptionAttempt) -> Result<(), AppError> {
        self.record_attempt(attempt, None)
    }

    pub fn record_attempt(
        &self,
        attempt: &TranscriptionAttempt,
        cost_usd: Option<f64>,
    ) -> Result<(), AppError> {
        self.record_attempt_inner(attempt, cost_usd, None)
    }

    pub fn record_attempt_for_model(
        &self,
        attempt: &TranscriptionAttempt,
        cost_usd: Option<f64>,
        model: &str,
    ) -> Result<(), AppError> {
        self.record_attempt_inner(attempt, cost_usd, Some(model))
    }

    fn record_attempt_inner(
        &self,
        attempt: &TranscriptionAttempt,
        cost_usd: Option<f64>,
        model: Option<&str>,
    ) -> Result<(), AppError> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let inserted = tx.execute(
            "INSERT INTO transcription_attempts (
                    id, recording_id, attempt_number, started_at, ended_at, outcome,
                    error_category, http_status, latency_ms
                ) SELECT ?1,?2,?3,?4,?5,?6,?7,?8,?9
                  WHERE EXISTS (SELECT 1 FROM recordings r WHERE r.id=?2
                                AND NOT EXISTS (SELECT 1 FROM usage_suppressed s WHERE s.recording_id=r.id))",
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
        if inserted == 0 {
            tx.commit()
                .map_err(|e| AppError::StorageFailed(e.to_string()))?;
            return Ok(());
        }
        tx.execute(
            "UPDATE recordings SET attempt_count = MAX(attempt_count, ?1), updated_at=?2 WHERE id=?3",
            params![
                attempt.attempt_number,
                Utc::now().to_rfc3339(),
                attempt.recording_id
            ],
        )
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        tx.execute("INSERT OR IGNORE INTO usage_attempts(attempt_id,recording_id,occurred_at,model,cost_usd) SELECT ?1,?2,?3,COALESCE(?5,model),?4 FROM recordings WHERE id=?2 AND NOT EXISTS(SELECT 1 FROM usage_suppressed WHERE recording_id=?2)", params![attempt.id, attempt.recording_id, attempt.ended_at.unwrap_or(attempt.started_at).to_rfc3339(), cost_usd, model]).map_err(storage_err)?;
        tx.commit()
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

fn ensure_search_text_column(conn: &Connection) -> Result<(), AppError> {
    let columns = {
        let mut stmt = conn
            .prepare("PRAGMA table_info(recordings)")
            .map_err(storage_err)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(storage_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(storage_err)?
    };
    if !columns.iter().any(|column| column == "search_text") {
        conn.execute(
            "ALTER TABLE recordings ADD COLUMN search_text TEXT NOT NULL DEFAULT ''",
            [],
        )
        .map_err(storage_err)?;
    }
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_recordings_search_text ON recordings(search_text)",
        [],
    )
    .map_err(storage_err)?;
    Ok(())
}

fn normalize_search_text(fields: &[&str]) -> String {
    let mut normalized = String::new();
    for ch in fields.iter().flat_map(|field| field.chars()) {
        match ch {
            '\u{00df}' | '\u{1e9e}' => normalized.push_str("ss"),
            '\u{03c2}' => normalized.push('\u{03c3}'),
            '\u{017f}' => normalized.push('s'),
            '\u{fb00}' => normalized.push_str("ff"),
            '\u{fb01}' => normalized.push_str("fi"),
            '\u{fb02}' => normalized.push_str("fl"),
            '\u{fb03}' => normalized.push_str("ffi"),
            '\u{fb04}' => normalized.push_str("ffl"),
            '\u{fb05}' | '\u{fb06}' => normalized.push_str("st"),
            _ => normalized.extend(ch.to_lowercase()),
        }
    }
    normalized
}

fn recording_search_text(rec: &Recording) -> String {
    normalize_search_text(&[
        rec.transcript.as_deref().unwrap_or_default(),
        rec.last_error_message.as_deref().unwrap_or_default(),
        rec.last_error_code.as_deref().unwrap_or_default(),
        &rec.model,
    ])
}

fn storage_err(error: rusqlite::Error) -> AppError {
    AppError::StorageFailed(error.to_string())
}

fn local_date(value: &str) -> String {
    DateTime::parse_from_rfc3339(value)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|_| value.chars().take(10).collect())
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
fn like_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

fn encode_cursor(rec: &Recording) -> String {
    format!("{}|{}", rec.created_at.to_rfc3339(), rec.id)
}

fn decode_cursor(repo: &HistoryRepo, cursor: &str) -> Result<Option<(String, String)>, AppError> {
    if let Some((created_at, id)) = cursor.split_once('|') {
        if !created_at.is_empty() && !id.is_empty() {
            return Ok(Some((created_at.to_string(), id.to_string())));
        }
    }
    repo.conn
        .query_row(
            "SELECT created_at, id FROM recordings WHERE id=?1",
            params![cursor],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage_err)
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

pub fn history_play_name(rec: &Recording, _keep_original: bool) -> Option<&String> {
    if rec.processed_audio_path.is_some() {
        return rec.processed_audio_path.as_ref();
    }
    rec.raw_audio_path.as_ref()
}

#[cfg(test)]
mod tests {
    #[test]
    fn summary_pages_search_and_legacy_cursor_skip_large_usage_payload() {
        let dir = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("summary.db")).unwrap();
        for index in 0..3 {
            let mut rec = new_recording("vendor/model".into());
            rec.id = format!("rec-{index}");
            rec.created_at = Utc::now() - chrono::Duration::seconds(index);
            rec.transcript = Some("Привет %_".into());
            repo.insert(&rec).unwrap();
            repo.update(&rec).unwrap();
        }
        // A blob cannot be decoded by the full Recording String mapper. Summary
        // queries must neither select nor materialize this large detail payload.
        repo.conn
            .execute("UPDATE recordings SET usage_json=zeroblob(1048576)", [])
            .unwrap();
        assert!(repo.get("rec-0").is_err());
        let first = repo
            .list_summaries_page(None, 1, Some("привет %_"))
            .unwrap();
        assert_eq!(first.total, 3);
        assert!(first.has_more);
        assert_eq!(first.items[0].id, "rec-0");
        let second = repo
            .list_summaries_page(first.next_cursor.as_deref(), 1, Some("ПРИВЕТ %_"))
            .unwrap();
        assert_eq!(second.items[0].id, "rec-1");
        assert_eq!(second.total, 3);
        let last = repo
            .list_summaries_page(Some("rec-1"), 1, Some("привет %_"))
            .unwrap();
        assert_eq!(last.items[0].id, "rec-2");
        assert!(!last.has_more);
        assert!(last.next_cursor.is_none());
        assert_eq!(repo.list_summaries(200).unwrap().len(), 3);
        assert_eq!(
            repo.list_summaries_page(Some("missing"), 1, None)
                .unwrap()
                .items[0]
                .id,
            "rec-0"
        );
    }

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
    fn failed_usage_insert_rolls_back_recording_row() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("atomic-insert.db")).unwrap();
        repo.conn
            .execute_batch(
                "CREATE TRIGGER reject_usage_insert BEFORE INSERT ON usage_dictations BEGIN SELECT RAISE(ABORT, 'injected usage failure'); END;",
            )
            .unwrap();
        let rec = new_recording("vendor/model-a".into());

        assert!(repo.insert(&rec).is_err());
        assert!(repo.get(&rec.id).unwrap().is_none());
        assert_eq!(repo.count(None).unwrap(), 0);
    }

    #[test]
    fn failed_usage_update_rolls_back_recording_changes() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("atomic-update.db")).unwrap();
        let mut rec = new_recording("vendor/model-before".into());
        rec.duration_ms = 10;
        repo.insert(&rec).unwrap();
        repo.conn
            .execute_batch(
                "CREATE TRIGGER reject_usage_update BEFORE UPDATE ON usage_dictations BEGIN SELECT RAISE(ABORT, 'injected usage update failure'); END;",
            )
            .unwrap();
        rec.status = RecordingStatus::Completed;
        rec.model = "vendor/model-after".into();
        rec.duration_ms = 99;

        assert!(repo.update(&rec).is_err());
        let saved = repo.get(&rec.id).unwrap().unwrap();
        assert_eq!(saved.status, RecordingStatus::Processing);
        assert_eq!(saved.model, "vendor/model-before");
        assert_eq!(saved.duration_ms, 10);
        let usage = repo
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!(usage.completed, 0);
        assert_eq!(usage.audio_duration_ms, 10);
        assert_eq!(usage.models[0].model, "vendor/model-before");
    }

    #[test]
    fn cancelled_recording_tombstone_hides_data_and_removes_usage_after_delete_failure() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("cancelled-tombstone.db")).unwrap();
        let mut rec = new_recording("vendor/model-cancelled".into());
        rec.status = RecordingStatus::Processing;
        rec.transcript = Some("must not remain visible".into());
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        repo.insert(&rec).unwrap();
        repo.record_attempt(
            &TranscriptionAttempt {
                id: "cancelled-tombstone-attempt".into(),
                recording_id: rec.id.clone(),
                attempt_number: 1,
                started_at: rec.created_at,
                ended_at: Some(rec.created_at),
                outcome: "success".into(),
                error_category: None,
                http_status: Some(200),
                latency_ms: Some(10),
            },
            Some(0.5),
        )
        .unwrap();
        repo.conn
            .execute_batch(
                "CREATE TRIGGER reject_cancelled_delete BEFORE DELETE ON recordings BEGIN SELECT RAISE(ABORT, 'injected delete failure'); END;",
            )
            .unwrap();
        assert!(repo.discard_cancelled_recording(&rec.id).is_err());

        assert!(repo.suppress_cancelled_recording(&rec.id).unwrap());
        let tombstone = repo.get(&rec.id).unwrap().unwrap();
        assert_eq!(tombstone.last_error_code.as_deref(), Some("Cancelled"));
        assert_eq!(tombstone.transcript, None);
        assert_eq!(tombstone.raw_audio_path, None);
        assert_eq!(repo.count(None).unwrap(), 0);
        assert!(repo.list_attempts(&rec.id).unwrap().is_empty());
        let stats = repo
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!((stats.dictations, stats.api_requests), (0, 0));
        assert_eq!(stats.reported_cost_usd, 0.0);

        repo.record_attempt(
            &TranscriptionAttempt {
                id: "late-before-physical-delete".into(),
                recording_id: rec.id.clone(),
                attempt_number: 2,
                started_at: rec.created_at,
                ended_at: Some(rec.created_at),
                outcome: "success".into(),
                error_category: None,
                http_status: Some(200),
                latency_ms: Some(5),
            },
            Some(0.25),
        )
        .unwrap();
        assert!(repo.list_attempts(&rec.id).unwrap().is_empty());
        repo.conn
            .execute_batch("DROP TRIGGER reject_cancelled_delete;")
            .unwrap();
        repo.discard_cancelled_recording(&rec.id).unwrap();
        repo.record_attempt(
            &TranscriptionAttempt {
                id: "late-after-physical-delete".into(),
                recording_id: rec.id.clone(),
                attempt_number: 2,
                started_at: rec.created_at,
                ended_at: Some(rec.created_at),
                outcome: "success".into(),
                error_category: None,
                http_status: Some(200),
                latency_ms: Some(5),
            },
            Some(0.25),
        )
        .unwrap();
        assert!(repo.list_attempts(&rec.id).unwrap().is_empty());
        assert!(repo.get(&rec.id).unwrap().is_none());
    }

    #[test]
    fn legacy_attempt_model_is_unknown_when_original_attempt_model_was_not_recorded() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("legacy-attempt-model.db");
        let repo = HistoryRepo::open(&path).unwrap();
        let mut rec = new_recording("vendor/model-first".into());
        repo.insert(&rec).unwrap();
        repo.record_attempt(
            &TranscriptionAttempt {
                id: "legacy-model-attempt".into(),
                recording_id: rec.id.clone(),
                attempt_number: 1,
                started_at: rec.created_at,
                ended_at: Some(rec.created_at),
                outcome: "failed".into(),
                error_category: Some("provider".into()),
                http_status: Some(503),
                latency_ms: Some(10),
            },
            None,
        )
        .unwrap();
        rec.model = "vendor/model-retried".into();
        repo.update(&rec).unwrap();
        repo.conn
            .execute_batch("DELETE FROM usage_attempts; DELETE FROM usage_dictations; DELETE FROM usage_meta WHERE key='backfill-v1';")
            .unwrap();
        drop(repo);

        let repo = HistoryRepo::open(&path).unwrap();
        let stats = repo
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        let legacy_attempt = stats
            .models
            .iter()
            .find(|model| model.api_requests == 1)
            .unwrap();
        assert_eq!(legacy_attempt.model, "unknown");
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
        assert_eq!(
            history_play_name(&rec, false).map(String::as_str),
            Some("1.wav")
        );
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
        repo.delete(&page.items[2].id).unwrap();
        let after_delete = repo
            .list_page(page.next_cursor.as_deref(), 3, None)
            .unwrap();
        assert!(!after_delete.items.is_empty());
        assert!(after_delete
            .items
            .iter()
            .all(|item| item.id != page.items[2].id));
        let literal = repo.list_page(None, 10, Some("%")).unwrap();
        assert_eq!(literal.total, 0);
    }

    #[test]
    fn unicode_search_matches_count_and_all_cursor_pages_consistently() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        for (i, text) in [
            "ПРОВЕРКА голоса",
            "проверка ГОЛОСА",
            "MixedCase Latin",
            "under_score 100% done",
            "unrelated",
            "Straße Ος",
        ]
        .into_iter()
        .enumerate()
        {
            let mut rec = new_recording("Vendor/Model-X".into());
            rec.id = format!("search-{i}");
            rec.transcript = Some(text.into());
            rec.created_at -= chrono::Duration::seconds(i as i64);
            rec.updated_at = rec.created_at;
            repo.insert(&rec).unwrap();
        }

        let first = repo.list_page(None, 1, Some("ГоЛоСа")).unwrap();
        assert_eq!(first.total, 2);
        assert_eq!(first.items.len(), 1);
        assert!(first.has_more);
        let second = repo
            .list_page(first.next_cursor.as_deref(), 1, Some("голоса"))
            .unwrap();
        assert_eq!(second.total, first.total);
        assert_eq!(second.items.len(), 1);
        assert!(!second.has_more);
        assert_ne!(first.items[0].id, second.items[0].id);

        assert_eq!(repo.count(Some("lAtIn")).unwrap(), 1);
        assert_eq!(repo.list_page(None, 10, Some("100%")).unwrap().total, 1);
        assert_eq!(
            repo.list_page(None, 10, Some("under_score")).unwrap().total,
            1
        );
        assert_eq!(
            repo.list_page(None, 10, Some("underXscore")).unwrap().total,
            0
        );
        assert_eq!(repo.count(Some("STRASSE")).unwrap(), 1);
        assert_eq!(repo.list_page(None, 10, Some("οσ")).unwrap().total, 1);
    }

    #[test]
    fn search_text_is_backfilled_after_schema_upgrade() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("h.db");
        let repo = HistoryRepo::open(&path).unwrap();
        let mut rec = new_recording("Vendor/Model".into());
        rec.transcript = Some("Старый ТЕКСТ".into());
        repo.insert(&rec).unwrap();
        drop(repo);

        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "DROP INDEX idx_recordings_search_text;
             ALTER TABLE recordings DROP COLUMN search_text;
             DELETE FROM usage_meta WHERE key='search-fold-v2';",
        )
        .unwrap();
        drop(conn);

        let migrated = HistoryRepo::open(&path).unwrap();
        assert_eq!(migrated.count(Some("СТАРЫЙ текст")).unwrap(), 1);
    }

    #[test]
    fn record_attempt_bumps_count_in_one_transaction() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("h.db")).unwrap();
        let rec = new_recording("m".into());
        repo.insert(&rec).unwrap();
        repo.record_attempt(
            &TranscriptionAttempt {
                id: "a1".into(),
                recording_id: rec.id.clone(),
                attempt_number: 2,
                started_at: rec.created_at,
                ended_at: Some(rec.created_at),
                outcome: "success".into(),
                error_category: None,
                http_status: Some(200),
                latency_ms: Some(12),
            },
            None,
        )
        .unwrap();
        let kept = repo.get(&rec.id).unwrap().unwrap();
        assert_eq!(kept.attempt_count, 2);
        assert_eq!(repo.list_attempts(&rec.id).unwrap().len(), 1);
        assert!(repo.delete(&rec.id).unwrap());
        assert!(repo.get(&rec.id).unwrap().is_none());
        assert!(repo.list_attempts(&rec.id).unwrap().is_empty());
    }

    #[test]
    fn usage_counts_retries_partial_cost_and_manual_delete_separately() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("usage.db")).unwrap();
        let mut rec = new_recording("vendor/model-a".into());
        rec.status = RecordingStatus::Completed;
        repo.insert(&rec).unwrap();
        repo.update(&rec).unwrap();
        for (id, number, cost) in [("a1", 1, None), ("a2", 2, None), ("a3", 3, Some(0.125))] {
            repo.record_attempt(
                &TranscriptionAttempt {
                    id: id.into(),
                    recording_id: rec.id.clone(),
                    attempt_number: number,
                    started_at: rec.created_at,
                    ended_at: Some(rec.created_at),
                    outcome: if number == 3 { "success" } else { "error" }.into(),
                    error_category: None,
                    http_status: Some(200),
                    latency_ms: Some(10),
                },
                cost,
            )
            .unwrap();
        }
        let end = rec.created_at + chrono::Duration::seconds(1);
        let stats = repo.get_usage_statistics(rec.created_at, end).unwrap();
        assert_eq!(
            (
                stats.dictations,
                stats.completed,
                stats.api_requests,
                stats.unpriced_attempts
            ),
            (1, 1, 3, 2)
        );
        assert!((stats.reported_cost_usd - 0.125).abs() < 0.0001);
        let exclusive = repo
            .get_usage_statistics(rec.created_at, rec.created_at)
            .unwrap();
        assert_eq!(exclusive.api_requests, 0);
        repo.delete(&rec.id).unwrap(); // Automatic detail retention preserves usage.
        assert_eq!(
            repo.get_usage_statistics(rec.created_at, end)
                .unwrap()
                .dictations,
            1
        );
        repo.delete_usage_for_recording(&rec.id).unwrap();
        assert_eq!(
            repo.get_usage_statistics(rec.created_at, end)
                .unwrap()
                .api_requests,
            0
        );
    }

    #[test]
    fn manual_delete_removes_history_and_usage_in_one_transaction() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("atomic-delete.db")).unwrap();
        let mut rec = new_recording("vendor/model-delete".into());
        rec.created_at = Utc::now() - chrono::Duration::seconds(2);
        repo.insert(&rec).unwrap();
        repo.record_attempt(
            &TranscriptionAttempt {
                id: "atomic-delete-attempt".into(),
                recording_id: rec.id.clone(),
                attempt_number: 1,
                started_at: rec.created_at,
                ended_at: Some(rec.created_at),
                outcome: "failed".into(),
                error_category: Some("network".into()),
                http_status: None,
                latency_ms: Some(10),
            },
            None,
        )
        .unwrap();
        repo.conn
            .execute_batch(
                "CREATE TRIGGER reject_usage_delete BEFORE DELETE ON usage_attempts \
                 BEGIN SELECT RAISE(ABORT, 'injected delete failure'); END;",
            )
            .unwrap();

        assert!(repo.delete_with_usage(&rec.id).is_err());
        assert!(repo.get(&rec.id).unwrap().is_some());
        assert_eq!(repo.list_attempts(&rec.id).unwrap().len(), 1);
        let stats = repo
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!((stats.dictations, stats.api_requests), (1, 1));

        repo.conn
            .execute_batch("DROP TRIGGER reject_usage_delete;")
            .unwrap();
        assert!(repo.delete_with_usage(&rec.id).unwrap());
        assert!(repo.get(&rec.id).unwrap().is_none());
        assert!(repo.list_attempts(&rec.id).unwrap().is_empty());
        let stats = repo
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!((stats.dictations, stats.api_requests), (0, 0));
    }

    #[test]
    fn deleting_history_and_pruning_remove_orphaned_suppression_markers() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("usage-suppression.db")).unwrap();
        let rec = new_recording("vendor/model-suppressed".into());
        repo.insert(&rec).unwrap();
        repo.clear_usage_statistics().unwrap();
        assert_eq!(
            repo.conn
                .query_row(
                    "SELECT COUNT(*) FROM usage_suppressed WHERE recording_id=?1",
                    params![rec.id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );

        assert!(repo.delete(&rec.id).unwrap());
        repo.conn
            .execute(
                "INSERT INTO usage_suppressed(recording_id) VALUES('orphaned-id')",
                [],
            )
            .unwrap();
        repo.prune_usage_statistics(Utc::now() - chrono::Duration::days(90))
            .unwrap();
        assert_eq!(
            repo.conn
                .query_row("SELECT COUNT(*) FROM usage_suppressed", [], |row| row
                    .get::<_, i64>(0),)
                .unwrap(),
            0
        );
    }

    #[test]
    fn legacy_dictation_cost_is_present_in_daily_and_model_buckets() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("legacy-cost.db");
        let repo = HistoryRepo::open(&path).unwrap();
        let mut rec = new_recording("vendor/legacy-model".into());
        rec.status = RecordingStatus::Completed;
        rec.cost = Some(0.125);
        repo.insert(&rec).unwrap();
        repo.update(&rec).unwrap();

        // Simulate a pre-ledger database with a known final cost but no attempt rows.
        repo.conn
            .execute("DELETE FROM usage_dictations", [])
            .unwrap();
        repo.conn
            .execute("DELETE FROM usage_meta WHERE key='backfill-v1'", [])
            .unwrap();
        drop(repo);

        let repo = HistoryRepo::open(&path).unwrap();
        let stats = repo
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert!((stats.reported_cost_usd - 0.125).abs() < 0.0001);
        assert_eq!(stats.daily.len(), 1);
        assert!((stats.daily[0].reported_cost_usd - 0.125).abs() < 0.0001);
        assert_eq!(stats.models.len(), 1);
        assert_eq!(stats.models[0].model, rec.model);
        assert!((stats.models[0].reported_cost_usd - 0.125).abs() < 0.0001);
    }

    #[test]
    fn retry_model_change_keeps_legacy_cost_with_original_model() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("legacy-retry-cost.db");
        let repo = HistoryRepo::open(&path).unwrap();
        let mut rec = new_recording("vendor/model-a".into());
        rec.status = RecordingStatus::Completed;
        rec.cost = Some(0.125);
        repo.insert(&rec).unwrap();
        repo.update(&rec).unwrap();

        // Recreate an old database with dictation-level cost and no attempt ledger.
        repo.conn
            .execute("DELETE FROM usage_dictations", [])
            .unwrap();
        repo.conn
            .execute("DELETE FROM usage_meta WHERE key='backfill-v1'", [])
            .unwrap();
        drop(repo);

        let repo = HistoryRepo::open(&path).unwrap();
        let attempt_at = rec.created_at + chrono::Duration::seconds(1);
        repo.record_attempt_for_model(
            &TranscriptionAttempt {
                id: "model-b-retry".into(),
                recording_id: rec.id.clone(),
                attempt_number: 2,
                started_at: attempt_at,
                ended_at: Some(attempt_at),
                outcome: "success".into(),
                error_category: None,
                http_status: Some(200),
                latency_ms: Some(10),
            },
            Some(0.2),
            "vendor/model-b",
        )
        .unwrap();
        rec.model = "vendor/model-b".into();
        rec.updated_at = attempt_at;
        repo.update(&rec).unwrap();

        let stats = repo
            .get_usage_statistics(rec.created_at, attempt_at + chrono::Duration::seconds(1))
            .unwrap();
        assert!((stats.reported_cost_usd - 0.325).abs() < 0.0001);
        let model_a = stats
            .models
            .iter()
            .find(|model| model.model == "vendor/model-a")
            .unwrap();
        let model_b = stats
            .models
            .iter()
            .find(|model| model.model == "vendor/model-b")
            .unwrap();
        assert!((model_a.reported_cost_usd - 0.125).abs() < 0.0001);
        assert!((model_b.reported_cost_usd - 0.2).abs() < 0.0001);
    }

    #[test]
    fn usage_models_report_unpriced_attempts_per_model() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("model-costs.db")).unwrap();
        let mut priced = new_recording("vendor/model-priced".into());
        priced.created_at = Utc::now() - chrono::Duration::seconds(2);
        repo.insert(&priced).unwrap();
        let mut unpriced = new_recording("vendor/model-unpriced".into());
        unpriced.created_at = priced.created_at + chrono::Duration::seconds(1);
        repo.insert(&unpriced).unwrap();

        for (recording, attempt_id, cost) in [
            (&priced, "priced-attempt", Some(0.125)),
            (&unpriced, "unpriced-attempt", None),
        ] {
            repo.record_attempt(
                &TranscriptionAttempt {
                    id: attempt_id.into(),
                    recording_id: recording.id.clone(),
                    attempt_number: 1,
                    started_at: recording.created_at,
                    ended_at: Some(recording.created_at),
                    outcome: "success".into(),
                    error_category: None,
                    http_status: Some(200),
                    latency_ms: Some(10),
                },
                cost,
            )
            .unwrap();
        }

        let stats = repo
            .get_usage_statistics(
                priced.created_at,
                unpriced.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        let priced_model = stats
            .models
            .iter()
            .find(|model| model.model == priced.model)
            .unwrap();
        let unpriced_model = stats
            .models
            .iter()
            .find(|model| model.model == unpriced.model)
            .unwrap();

        assert_eq!(priced_model.api_requests, 1);
        assert_eq!(priced_model.unpriced_attempts, 0);
        assert_eq!(unpriced_model.api_requests, 1);
        assert_eq!(unpriced_model.unpriced_attempts, 1);
    }

    #[test]
    fn cancelled_recording_removes_all_history_and_usage_atomically() {
        let dir = tempdir().unwrap();
        let repo = HistoryRepo::open(&dir.path().join("cancelled.db")).unwrap();
        let mut rec = new_recording("vendor/model-cancelled".into());
        rec.status = RecordingStatus::Completed;
        repo.insert(&rec).unwrap();
        repo.update(&rec).unwrap();
        repo.record_attempt(
            &TranscriptionAttempt {
                id: "cancel-attempt".into(),
                recording_id: rec.id.clone(),
                attempt_number: 1,
                started_at: rec.created_at,
                ended_at: Some(rec.created_at),
                outcome: "success".into(),
                error_category: None,
                http_status: Some(200),
                latency_ms: Some(10),
            },
            Some(0.25),
        )
        .unwrap();

        assert!(repo.discard_cancelled_recording(&rec.id).unwrap().is_some());
        assert!(repo.get(&rec.id).unwrap().is_none());
        assert!(repo.list_attempts(&rec.id).unwrap().is_empty());
        let stats = repo
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!((stats.dictations, stats.api_requests), (0, 0));
        assert_eq!(stats.reported_cost_usd, 0.0);

        // A late in-flight attempt cannot recreate usage after its parent row is gone.
        assert!(repo
            .record_attempt(
                &TranscriptionAttempt {
                    id: "late-cancel-attempt".into(),
                    recording_id: rec.id.clone(),
                    attempt_number: 2,
                    started_at: Utc::now(),
                    ended_at: Some(Utc::now()),
                    outcome: "success".into(),
                    error_category: None,
                    http_status: Some(200),
                    latency_ms: Some(10),
                },
                Some(1.0),
            )
            .is_ok());
        assert!(repo.list_attempts(&rec.id).unwrap().is_empty());
        let stats = repo
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!((stats.dictations, stats.api_requests), (0, 0));
        assert_eq!(stats.reported_cost_usd, 0.0);
    }

    #[test]
    fn startup_migration_keeps_hidden_cancelled_tombstone_and_recovers_cancelled_retry() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("legacy-cancelled.db");
        let repo = HistoryRepo::open(&path).unwrap();

        let mut cancelled = new_recording("vendor/model-cancelled".into());
        cancelled.status = RecordingStatus::Failed;
        cancelled.last_error_code = Some("Cancelled".into());
        repo.insert(&cancelled).unwrap();
        repo.update(&cancelled).unwrap();

        let mut cancelled_retry = new_recording("vendor/model-previous-success".into());
        cancelled_retry.status = RecordingStatus::Completed;
        cancelled_retry.transcript = Some("previous transcript".into());
        cancelled_retry.completed_at = Some(cancelled_retry.created_at);
        cancelled_retry.cost = Some(0.25);
        cancelled_retry.usage_json = Some(r#"{"total_tokens":12}"#.into());
        repo.insert(&cancelled_retry).unwrap();
        repo.update(&cancelled_retry).unwrap();
        repo.record_attempt(
            &TranscriptionAttempt {
                id: "previous-success-attempt".into(),
                recording_id: cancelled_retry.id.clone(),
                attempt_number: 1,
                started_at: cancelled_retry.created_at,
                ended_at: Some(cancelled_retry.created_at),
                outcome: "success".into(),
                error_category: None,
                http_status: Some(200),
                latency_ms: Some(10),
            },
            Some(0.25),
        )
        .unwrap();
        cancelled_retry.status = RecordingStatus::Failed;
        cancelled_retry.last_error_code = Some("Cancelled".into());
        cancelled_retry.last_error_message = Some("Cancelled".into());
        repo.update(&cancelled_retry).unwrap();

        let mut legacy_cancel = new_recording("vendor/model-cancelled".into());
        legacy_cancel.status = RecordingStatus::Interrupted;
        repo.insert(&legacy_cancel).unwrap();
        assert!(repo.delete(&legacy_cancel.id).unwrap());

        let mut expired_detail = new_recording("vendor/model-completed".into());
        expired_detail.status = RecordingStatus::Completed;
        repo.insert(&expired_detail).unwrap();
        repo.update(&expired_detail).unwrap();
        assert!(repo.delete(&expired_detail.id).unwrap());
        repo.conn
            .execute("DELETE FROM usage_meta WHERE key='remove-cancelled-v2'", [])
            .unwrap();
        drop(repo);

        let repo = HistoryRepo::open(&path).unwrap();
        let tombstone = repo.get(&cancelled.id).unwrap().unwrap();
        assert_eq!(tombstone.last_error_code.as_deref(), Some("Cancelled"));
        assert_eq!(tombstone.transcript, None);
        assert_eq!(tombstone.raw_audio_path, None);
        assert_eq!(tombstone.processed_audio_path, None);
        assert_eq!(repo.count(None).unwrap(), 1);
        let recovered = repo.get(&cancelled_retry.id).unwrap().unwrap();
        assert_eq!(recovered.status, RecordingStatus::Completed);
        assert_eq!(recovered.transcript.as_deref(), Some("previous transcript"));
        assert_eq!(recovered.last_error_code, None);
        assert_eq!(recovered.completed_at, cancelled_retry.completed_at);
        assert_eq!(recovered.cost, Some(0.25));
        assert!(repo
            .list_attempts(&cancelled_retry.id)
            .unwrap()
            .iter()
            .any(|attempt| {
                attempt.id == "previous-success-attempt" && attempt.outcome == "success"
            }));
        let stats = repo
            .get_usage_statistics(
                cancelled.created_at.min(expired_detail.created_at),
                Utc::now() + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!(stats.dictations, 2);
        assert_eq!(stats.interrupted, 0);
        assert_eq!(stats.completed, 2);
        assert_eq!(stats.reported_cost_usd, 0.25);
    }

    #[test]
    fn usage_expiry_and_full_clear_survive_restart() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("usage.db");
        let repo = HistoryRepo::open(&path).unwrap();
        let mut fresh = new_recording("vendor/fresh".into());
        fresh.attempt_count = 2;
        repo.insert(&fresh).unwrap();
        let mut old = new_recording("vendor/old".into());
        old.created_at = Utc::now() - chrono::Duration::days(91);
        repo.insert(&old).unwrap();
        repo.conn
            .execute("DELETE FROM usage_dictations", [])
            .unwrap();
        repo.conn.execute("DELETE FROM usage_meta", []).unwrap();
        drop(repo);
        let repo = HistoryRepo::open(&path).unwrap();
        assert_eq!(
            repo.get_usage_statistics(
                fresh.created_at,
                fresh.created_at + chrono::Duration::seconds(1)
            )
            .unwrap()
            .dictations,
            1
        );
        assert_eq!(
            repo.get_usage_statistics(
                fresh.created_at,
                fresh.created_at + chrono::Duration::seconds(1)
            )
            .unwrap()
            .api_requests,
            2
        );
        drop(repo);
        let repo = HistoryRepo::open(&path).unwrap();
        assert_eq!(
            repo.get_usage_statistics(
                fresh.created_at,
                fresh.created_at + chrono::Duration::seconds(1)
            )
            .unwrap()
            .dictations,
            1
        );
        repo.prune_usage_statistics(Utc::now() - chrono::Duration::days(90))
            .unwrap();
        assert_eq!(
            repo.get_usage_statistics(
                old.created_at,
                old.created_at + chrono::Duration::seconds(1)
            )
            .unwrap()
            .dictations,
            0
        );
        repo.clear_usage_statistics().unwrap();
        repo.record_attempt(
            &TranscriptionAttempt {
                id: "after-clear".into(),
                recording_id: fresh.id.clone(),
                attempt_number: 1,
                started_at: Utc::now(),
                ended_at: Some(Utc::now()),
                outcome: "success".into(),
                error_category: None,
                http_status: Some(200),
                latency_ms: Some(1),
            },
            Some(0.5),
        )
        .unwrap();
        repo.update(&fresh).unwrap();
        drop(repo);
        let repo = HistoryRepo::open(&path).unwrap();
        assert_eq!(
            repo.get_usage_statistics(
                Utc::now() - chrono::Duration::days(90),
                Utc::now() + chrono::Duration::seconds(1)
            )
            .unwrap()
            .dictations,
            0
        );
    }
}
