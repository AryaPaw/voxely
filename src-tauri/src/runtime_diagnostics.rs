//! Bounded, best-effort lifecycle evidence. Absence of an exit record is not a crash diagnosis.
//! This journal deliberately has no API for arbitrary messages, errors, or panic payloads.

use std::cell::Cell;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_FILES: usize = 8;
const MAX_FILE_BYTES: u64 = 256 * 1024;
static JOURNAL: OnceLock<Journal> = OnceLock::new();
thread_local! {
    static IN_PANIC_HOOK: Cell<bool> = const { Cell::new(false) };
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExitReason {
    TrayQuit,
    MainWindowClose,
    UpdateRestart,
    UpdateInstaller,
    LocalRebuild,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NativeWindowMessage {
    SystemClose,
    Close,
    QueryEndSession,
    EndSession,
    Destroy,
    NonClientDestroy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LocalRebuildRefusal {
    NotLocal,
    InvalidTarget,
    NotReady,
    Busy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExitCompletion {
    TauriExitEvent,
    RunReturned,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ShutdownReason {
    CaptureCleanupTimeout,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UpdateOutcome {
    None,
    Available,
    Installed,
    Busy,
    Deferred,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PreviousRunStatus {
    CompletionRecorded,
    NoCompletionRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "name", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Event {
    ProcessStart,
    PreviousRunObserved {
        run_id: Uuid,
        status: PreviousRunStatus,
    },
    SetupStarted,
    SetupCompleted,
    Ready,
    StartupFailed,
    MainWindowHidden,
    MainWindowHideResult {
        hidden: bool,
        taskbar_hidden: bool,
    },
    MainWindowShown {
        shown: bool,
    },
    ClosePolicyLoaded {
        close_to_tray: bool,
    },
    ClosePolicyChanged {
        close_to_tray: bool,
        write_seq: u64,
    },
    MainWindowCloseDecision {
        close_to_tray: bool,
    },
    NativeWindowHookInstalled {
        succeeded: bool,
    },
    NativeWindowMessage {
        message: NativeWindowMessage,
        send_flags: u32,
        foreground: bool,
        session_ending: bool,
    },
    LocalRebuildRefused {
        reason: LocalRebuildRefusal,
    },
    ExitIntent {
        reason: ExitReason,
    },
    // Tauri does not provide the originating cause here; code is only the exit code.
    ExitRequested {
        code: Option<i32>,
    },
    ExitDeferred,
    ShutdownStarted,
    ShutdownWorkerUnavailable,
    UncoordinatedRestart,
    ShutdownCleanupDeferred,
    ShutdownCleanupReady,
    ForcedShutdownRequested {
        reason: ShutdownReason,
    },
    ExitComplete {
        source: ExitCompletion,
    },
    UpdateCheckStarted,
    UpdateCheckFinished {
        outcome: UpdateOutcome,
    },
    UpdateDownloadStarted,
    UpdateDownloadCompleted,
    UpdateInstallStarted,
    UpdateInstallReturned {
        succeeded: bool,
    },
    Panic {
        source_file: Option<String>,
        line: Option<u32>,
        column: Option<u32>,
        thread_id: String,
        thread_named: bool,
    },
}

#[derive(Serialize)]
struct Record<'a> {
    schema_version: u8,
    time_utc: String,
    process_id: u32,
    run_id: Uuid,
    version: &'static str,
    build_date: &'static str,
    local_build: bool,
    event: &'a Event,
}

#[derive(Deserialize)]
struct ReadRecord {
    run_id: Uuid,
    event: Event,
}

struct Journal {
    directory: PathBuf,
    run_id: Uuid,
    max_files: usize,
    max_bytes: u64,
    state: Mutex<Writer>,
}

struct Writer {
    file: Option<File>,
    bytes: u64,
    disabled: bool,
}

/// Call after resolving the existing application data directory, before loading app state.
/// Storage failure never changes startup or exit behavior.
pub(crate) fn initialize(data_directory: &Path) {
    if JOURNAL.get().is_some() {
        return;
    }
    match Journal::open(
        data_directory.join("diagnostics"),
        MAX_FILES,
        MAX_FILE_BYTES,
    ) {
        Ok(journal) => {
            if JOURNAL.set(journal).is_ok() {
                install_panic_hook();
            }
        }
        Err(_) => {
            // The logging subscriber may not exist yet. Never print a path or OS error here.
            eprintln!("Voxely runtime diagnostics unavailable");
        }
    }
}

pub(crate) fn record(event: Event) {
    if let Some(journal) = JOURNAL.get() {
        let _ = journal.record(&event);
    }
}

/// Native window callbacks must never wait on a journal write already in progress.
pub(crate) fn try_record(event: Event) {
    if let Some(journal) = JOURNAL.get() {
        let _ = journal.try_record(&event);
    }
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let entered = IN_PANIC_HOOK
            .try_with(|active| !active.replace(true))
            .unwrap_or(false);
        if entered {
            if let Some(journal) = JOURNAL.get() {
                let thread = std::thread::current();
                let event = panic_event(
                    info.location()
                        .map(|location| (location.file(), location.line(), location.column())),
                    &thread,
                );
                // A panic may have happened while writing this very journal. Never wait on it.
                let _ = journal.try_record(&event);
            }
        }
        // Preserve the existing hook and Rust's original unwind/abort behavior.
        previous(info);
        if entered {
            let _ = IN_PANIC_HOOK.try_with(|active| active.set(false));
        }
    }));
}

fn panic_event(location: Option<(&str, u32, u32)>, thread: &std::thread::Thread) -> Event {
    Event::Panic {
        // Compile paths can include a developer's profile directory. Keep only the basename.
        source_file: location.map(|(file, _, _)| {
            file.rsplit(['/', '\\'])
                .next()
                .unwrap_or("")
                .chars()
                .take(128)
                .collect()
        }),
        line: location.map(|(_, line, _)| line),
        column: location.map(|(_, _, column)| column),
        thread_id: format!("{:?}", thread.id()),
        // Thread names can be dynamically constructed from private data.
        thread_named: thread.name().is_some(),
    }
}

impl Journal {
    fn open(directory: PathBuf, max_files: usize, max_bytes: u64) -> io::Result<Self> {
        fs::create_dir_all(&directory)?;
        let previous = previous_run(&directory, max_files, max_bytes);
        rotate(&directory, max_files)?;
        let journal = Self {
            run_id: Uuid::new_v4(),
            state: Mutex::new(Writer {
                file: Some(create_segment(&directory)?),
                bytes: 0,
                disabled: false,
            }),
            directory,
            max_files,
            max_bytes,
        };
        journal.record(&Event::ProcessStart)?;
        if let Some((run_id, status)) = previous {
            journal.record(&Event::PreviousRunObserved { run_id, status })?;
        }
        Ok(journal)
    }

    fn encode(&self, event: &Event) -> io::Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec(&Record {
            schema_version: 1,
            time_utc: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            process_id: std::process::id(),
            run_id: self.run_id,
            version: env!("CARGO_PKG_VERSION"),
            build_date: env!("VOXELY_BUILD_DATE"),
            local_build: crate::app::lifecycle::is_local_build(),
            event,
        })?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    fn record(&self, event: &Event) -> io::Result<()> {
        let mut writer = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.write(&mut writer, event)
    }

    fn try_record(&self, event: &Event) -> io::Result<()> {
        let mut writer = self
            .state
            .try_lock()
            .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "journal busy"))?;
        self.write(&mut writer, event)
    }

    fn write(&self, writer: &mut Writer, event: &Event) -> io::Result<()> {
        if writer.disabled {
            return Err(io::Error::other("journal disabled"));
        }
        let result = self.write_enabled(writer, event);
        if result.is_err() {
            writer.disabled = true;
        }
        result
    }

    fn write_enabled(&self, writer: &mut Writer, event: &Event) -> io::Result<()> {
        let bytes = self.encode(event)?;
        if bytes.len() as u64 > self.max_bytes {
            return Err(io::Error::other("journal record exceeds segment limit"));
        }
        if writer.bytes + bytes.len() as u64 > self.max_bytes {
            // Windows requires closing the handle before renaming its file.
            writer.file.take();
            rotate(&self.directory, self.max_files)?;
            writer.file = Some(create_segment(&self.directory)?);
            writer.bytes = 0;
        }
        let file = writer
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("journal file unavailable"))?;
        file.write_all(&bytes)?;
        file.sync_data()?;
        writer.bytes += bytes.len() as u64;
        Ok(())
    }
}

fn segment(directory: &Path, index: usize) -> PathBuf {
    directory.join(format!("runtime-journal-{index:02}.jsonl"))
}

fn create_segment(directory: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(segment(directory, 0))
}

fn is_regular_file(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(true),
        Ok(_) => Err(io::Error::other("journal segment is not a regular file")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn rotate(directory: &Path, max_files: usize) -> io::Result<()> {
    // Validate all owned slots before making changes; leave unrelated paths untouched.
    for index in 0..max_files {
        is_regular_file(&segment(directory, index))?;
    }
    let oldest = segment(directory, max_files - 1);
    if is_regular_file(&oldest)? {
        fs::remove_file(oldest)?;
    }
    for index in (0..max_files - 1).rev() {
        let source = segment(directory, index);
        if is_regular_file(&source)? {
            fs::rename(source, segment(directory, index + 1))?;
        }
    }
    Ok(())
}

fn previous_run(
    directory: &Path,
    max_files: usize,
    max_bytes: u64,
) -> Option<(Uuid, PreviousRunStatus)> {
    let mut latest_run = None;
    let mut status = PreviousRunStatus::NoCompletionRecord;
    for index in 0..max_files {
        let path = segment(directory, index);
        if !is_regular_file(&path).ok()? {
            continue;
        }
        let file = File::open(path).ok()?;
        let mut bytes = String::new();
        file.take(max_bytes).read_to_string(&mut bytes).ok()?;
        for line in bytes.lines().rev() {
            let Ok(record) = serde_json::from_str::<ReadRecord>(line) else {
                continue;
            };
            let run_id = *latest_run.get_or_insert(record.run_id);
            if record.run_id != run_id {
                return Some((run_id, status));
            }
            if matches!(record.event, Event::ExitComplete { .. }) {
                status = PreviousRunStatus::CompletionRecorded;
            }
        }
    }
    latest_run.map(|run_id| (run_id, status))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn records(directory: &Path, max_files: usize) -> Vec<serde_json::Value> {
        let mut records = Vec::new();
        for index in (0..max_files).rev() {
            let Ok(text) = fs::read_to_string(segment(directory, index)) else {
                continue;
            };
            records.extend(text.lines().map(|line| serde_json::from_str(line).unwrap()));
        }
        records
    }

    #[test]
    fn records_utc_build_identity_and_explicit_exit_reasons() {
        let temp = tempfile::tempdir().unwrap();
        let journal = Journal::open(temp.path().to_owned(), 8, MAX_FILE_BYTES).unwrap();
        journal.record(&Event::SetupStarted).unwrap();
        journal.record(&Event::Ready).unwrap();
        journal
            .record(&Event::ExitIntent {
                reason: ExitReason::TrayQuit,
            })
            .unwrap();
        journal
            .record(&Event::ExitRequested { code: Some(0) })
            .unwrap();
        journal
            .record(&Event::ExitComplete {
                source: ExitCompletion::TauriExitEvent,
            })
            .unwrap();
        let entries = records(temp.path(), 8);
        assert_eq!(entries.len(), 6);
        assert!(entries.iter().all(|entry| {
            entry["run_id"] == journal.run_id.to_string()
                && entry["version"] == env!("CARGO_PKG_VERSION")
                && entry["process_id"] == std::process::id()
                && entry["time_utc"].as_str().unwrap().ends_with('Z')
                && entry["schema_version"] == 1
        }));
        assert_eq!(entries[3]["event"]["reason"], "tray_quit");
        assert_eq!(entries[4]["event"]["name"], "exit_requested");
        assert!(entries[4]["event"].get("reason").is_none());
    }

    #[test]
    fn rotation_bounds_storage_and_preserves_unrelated_files() {
        let temp = tempfile::tempdir().unwrap();
        let unrelated = temp.path().join("runtime-journal-backup.jsonl");
        fs::write(&unrelated, "private unrelated file").unwrap();
        fs::write(segment(temp.path(), 99), "unowned slot").unwrap();
        let journal = Journal::open(temp.path().to_owned(), 3, 1024).unwrap();
        for _ in 0..40 {
            journal.record(&Event::Ready).unwrap();
        }
        assert!(records(temp.path(), 3).len() < 40);
        for index in 0..3 {
            assert!(fs::metadata(segment(temp.path(), index)).unwrap().len() <= 1024);
        }
        assert_eq!(
            fs::read_to_string(unrelated).unwrap(),
            "private unrelated file"
        );
        assert_eq!(
            fs::read_to_string(segment(temp.path(), 99)).unwrap(),
            "unowned slot"
        );
    }

    #[test]
    fn previous_run_reports_missing_marker_without_claiming_crash() {
        let temp = tempfile::tempdir().unwrap();
        let first = Journal::open(temp.path().to_owned(), 8, MAX_FILE_BYTES).unwrap();
        let first_id = first.run_id;
        first.record(&Event::Ready).unwrap();
        drop(first);
        let second = Journal::open(temp.path().to_owned(), 8, MAX_FILE_BYTES).unwrap();
        let entries = records(temp.path(), 8);
        let previous = entries.last().unwrap();
        assert_eq!(previous["event"]["run_id"], first_id.to_string());
        assert_eq!(previous["event"]["status"], "no_completion_record");
        second
            .record(&Event::ExitComplete {
                source: ExitCompletion::RunReturned,
            })
            .unwrap();
        drop(second);
        let _third = Journal::open(temp.path().to_owned(), 8, MAX_FILE_BYTES).unwrap();
        assert_eq!(
            records(temp.path(), 8).last().unwrap()["event"]["status"],
            "completion_recorded"
        );
    }

    #[test]
    fn ignores_truncated_tail_and_finds_completion_in_previous_segment() {
        let temp = tempfile::tempdir().unwrap();
        let journal = Journal::open(temp.path().to_owned(), 3, 1024).unwrap();
        journal
            .record(&Event::ExitComplete {
                source: ExitCompletion::TauriExitEvent,
            })
            .unwrap();
        journal.record(&Event::Ready).unwrap();
        journal.record(&Event::Ready).unwrap();
        let id = journal.run_id;
        drop(journal);
        OpenOptions::new()
            .append(true)
            .open(segment(temp.path(), 0))
            .unwrap()
            .write_all(b"{truncated")
            .unwrap();
        assert_eq!(
            previous_run(temp.path(), 3, 1024),
            Some((id, PreviousRunStatus::CompletionRecorded))
        );
    }

    #[test]
    fn panic_metadata_omits_payload_paths_and_thread_names() {
        let private = "secret-key-and-transcript";
        let thread = std::thread::Builder::new()
            .name(private.into())
            .spawn(|| {
                panic_event(
                    Some(("C:\\Users\\private-profile\\src\\module.rs", 12, 4)),
                    &std::thread::current(),
                )
            })
            .unwrap();
        let event = thread.join().unwrap();
        let encoded = serde_json::to_string(&event).unwrap();
        assert!(!encoded.contains(private));
        assert!(!encoded.contains("private-profile"));
        assert!(!encoded.contains("payload"));
        assert!(encoded.contains("module.rs"));
        assert!(encoded.contains("\"thread_named\":true"));
    }

    #[test]
    fn panic_hook_preserves_previous_hook_and_skips_busy_lock_in_isolated_process() {
        const CHILD_DIRECTORY: &str = "VOXELY_JOURNAL_PANIC_TEST_CHILD_DIRECTORY";
        if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
            use std::sync::atomic::{AtomicUsize, Ordering};
            use std::sync::Arc;

            let journal = Journal::open(PathBuf::from(directory), 8, MAX_FILE_BYTES).unwrap();
            assert!(JOURNAL.set(journal).is_ok());
            let called = Arc::new(AtomicUsize::new(0));
            let previous_called = Arc::clone(&called);
            std::panic::set_hook(Box::new(move |_| {
                previous_called.fetch_add(1, Ordering::SeqCst);
            }));
            install_panic_hook();
            let worker = std::thread::Builder::new()
                .name("private-dynamic-thread-name".into())
                .spawn(|| {
                    assert!(std::panic::catch_unwind(|| panic!("private-panic-payload")).is_err());
                })
                .unwrap();
            worker.join().unwrap();
            let journal = JOURNAL.get().unwrap();
            let held = journal.state.lock().unwrap();
            assert!(std::panic::catch_unwind(|| panic!("private-locked-panic")).is_err());
            drop(held);
            assert_eq!(called.load(Ordering::SeqCst), 2);
            return;
        }

        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};

        let temp = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "runtime_diagnostics::tests::panic_hook_preserves_previous_hook_and_skips_busy_lock_in_isolated_process",
            ])
            .env(CHILD_DIRECTORY, temp.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "isolated panic hook test failed: {status}"
                );
                break;
            }
            if started.elapsed() >= Duration::from_secs(10) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("isolated panic hook test exceeded its timeout");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let entries = records(temp.path(), 8);
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry["event"]["name"] == "panic")
                .count(),
            1
        );
        let encoded = serde_json::to_string(&entries).unwrap();
        assert!(!encoded.contains("private-panic-payload"));
        assert!(!encoded.contains("private-locked-panic"));
        assert!(!encoded.contains("private-dynamic-thread-name"));
    }

    #[test]
    fn storage_failure_and_busy_panic_lock_return_without_panicking() {
        let temp = tempfile::tempdir().unwrap();
        let blocked = temp.path().join("blocked");
        fs::write(&blocked, "preserved").unwrap();
        assert!(Journal::open(blocked, 8, MAX_FILE_BYTES).is_err());
        let journal = Journal::open(temp.path().join("working"), 8, MAX_FILE_BYTES).unwrap();
        let held = journal.state.lock().unwrap();
        assert_eq!(
            journal.try_record(&Event::Ready).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(held);
        journal.record(&Event::Ready).unwrap();
        assert_eq!(
            fs::read_to_string(temp.path().join("blocked")).unwrap(),
            "preserved"
        );
    }

    #[test]
    fn non_file_owned_slot_disables_journal_without_removing_other_data() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(segment(temp.path(), 4)).unwrap();
        fs::write(segment(temp.path(), 0), "existing journal").unwrap();
        assert!(Journal::open(temp.path().to_owned(), 8, MAX_FILE_BYTES).is_err());
        assert_eq!(
            fs::read_to_string(segment(temp.path(), 0)).unwrap(),
            "existing journal"
        );
        assert!(segment(temp.path(), 4).is_dir());
    }

    #[test]
    fn failed_write_disables_future_writes_without_changing_control_flow() {
        let temp = tempfile::tempdir().unwrap();
        let journal = Journal::open(temp.path().to_owned(), 8, MAX_FILE_BYTES).unwrap();
        journal.state.lock().unwrap().file.take();
        assert!(journal.record(&Event::Ready).is_err());
        assert!(journal.state.lock().unwrap().disabled);
        assert!(journal
            .record(&Event::ExitRequested { code: Some(0) })
            .is_err());
    }
}
