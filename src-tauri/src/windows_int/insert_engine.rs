use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

static INSERT_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeHwnd {
    pub value: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapturedTarget {
    pub hwnd: usize,
    pub root: usize,
    pub pid: u32,
    pub tid: u32,
    pub generation: u64,
}

impl CapturedTarget {
    pub fn from_hwnd(hwnd: NativeHwnd, root: usize, pid: u32, tid: u32, generation: u64) -> Self {
        Self {
            hwnd: hwnd.value,
            root,
            pid,
            tid,
            generation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyReason {
    UserSwitched,
    NoTarget,
    TargetGone,
    Minimized,
    IntegrityBlocked,
    SendBlocked,
    ClipboardMode,
    ClipboardBusy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertPolicy {
    SendUnicode,
    RestoreThenSend,
    CopyOnly(CopyReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertOutcome {
    Inserted,
    Copied { reason: CopyReason },
    PartialCopied,
    CancelledBeforeDelivery,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertKey {
    Unicode(u16),
    VirtualKey(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldSnapshot {
    pub captured: Option<CapturedTarget>,
    pub foreground_root: Option<usize>,
    pub foreground_pid: Option<u32>,
    pub focus_root: Option<usize>,
    pub overlay_root: Option<usize>,
    pub main_root: Option<usize>,
    pub target_alive: bool,
    pub target_iconic: bool,
    pub integrity_blocked: Option<bool>,
    pub window_class: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkSend {
    Complete,
    Zero,
    Partial { accepted: u32 },
    Odd { accepted: u32 },
}

pub trait InsertWorld {
    fn snapshot(&self, captured: Option<CapturedTarget>) -> WorldSnapshot;
    fn restore_foreground(&mut self, target: &CapturedTarget) -> bool;
    fn focus_caret(&mut self, target: &CapturedTarget) -> bool;
    fn wait_keys_up(&mut self, keys: &[u16], timeout: Duration);
    fn send_keys(&mut self, keys: &[InsertKey]) -> ChunkSend;
    fn clipboard_copy(&mut self, text: &str) -> Result<(), AppError>;
}

pub struct InsertRequest<'a> {
    pub mode: &'a str,
    pub text: &'a str,
    pub captured: Option<CapturedTarget>,
    pub overlay_root: Option<usize>,
    pub main_root: Option<usize>,
    pub hotkey: &'a str,
    pub abort: &'a dyn Fn() -> bool,
}

pub fn classify_insert_policy(snap: &WorldSnapshot) -> InsertPolicy {
    let Some(target) = snap.captured else {
        return InsertPolicy::CopyOnly(CopyReason::NoTarget);
    };
    if !snap.target_alive {
        return InsertPolicy::CopyOnly(CopyReason::TargetGone);
    }
    if snap.target_iconic {
        return InsertPolicy::CopyOnly(CopyReason::Minimized);
    }
    if snap.integrity_blocked == Some(true) {
        return InsertPolicy::CopyOnly(CopyReason::IntegrityBlocked);
    }
    match snap.foreground_root {
        Some(fg) if fg == target.root => InsertPolicy::SendUnicode,
        Some(fg) if snap.overlay_root == Some(fg) || snap.main_root == Some(fg) => {
            InsertPolicy::RestoreThenSend
        }
        Some(_) if snap.foreground_pid == Some(target.pid) => InsertPolicy::RestoreThenSend,
        Some(_) => InsertPolicy::CopyOnly(CopyReason::UserSwitched),
        None => InsertPolicy::CopyOnly(CopyReason::NoTarget),
    }
}

pub fn classify_chunk_send(accepted: u32, planned_events: u32) -> ChunkSend {
    if planned_events == 0 {
        return ChunkSend::Complete;
    }
    if accepted == 0 {
        return ChunkSend::Zero;
    }
    if accepted % 2 == 1 {
        return ChunkSend::Odd { accepted };
    }
    if accepted < planned_events {
        return ChunkSend::Partial { accepted };
    }
    ChunkSend::Complete
}

pub fn is_high_surrogate(unit: u16) -> bool {
    (0xD800..=0xDBFF).contains(&unit)
}

pub fn is_low_surrogate(unit: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&unit)
}

pub fn edit_class_uses_vk_return(class: &str) -> bool {
    let class = class.trim();
    class.eq_ignore_ascii_case("edit") || class.to_ascii_lowercase().starts_with("richedit")
}

pub fn plan_logical_units(text: &str, class: Option<&str>) -> Vec<Vec<InsertKey>> {
    let vk_return = class.is_some_and(edit_class_uses_vk_return);
    let units = text.encode_utf16().collect::<Vec<_>>();
    let mut out = Vec::new();
    let mut i = 0;
    while i < units.len() {
        let unit = units[i];
        if unit == 0x0D && units.get(i + 1) == Some(&0x0A) {
            out.push(vec![if vk_return {
                InsertKey::VirtualKey(0x0D)
            } else {
                InsertKey::Unicode(0x0A)
            }]);
            i += 2;
            continue;
        }
        if is_high_surrogate(unit) && units.get(i + 1).is_some_and(|low| is_low_surrogate(*low)) {
            out.push(vec![
                InsertKey::Unicode(unit),
                InsertKey::Unicode(units[i + 1]),
            ]);
            i += 2;
            continue;
        }
        out.push(vec![match unit {
            0x09 if vk_return => InsertKey::VirtualKey(0x09),
            0x0A | 0x0D if vk_return => InsertKey::VirtualKey(0x0D),
            _ => InsertKey::Unicode(unit),
        }]);
        i += 1;
    }
    out
}

pub fn plan_insert_units(text: &str) -> Vec<InsertKey> {
    plan_insert_units_for_class(text, None)
}

pub fn plan_insert_units_for_class(text: &str, class: Option<&str>) -> Vec<InsertKey> {
    plan_logical_units(text, class)
        .into_iter()
        .flatten()
        .collect()
}

pub const UNICODE_CHUNK_UNITS_DEFAULT: usize = 512;
pub const UNICODE_CHUNK_UNITS_MIN: usize = 256;
pub const UNICODE_CHUNK_UNITS_MAX: usize = 1024;
pub const UNICODE_CHUNK_UNITS_CHROMIUM: usize = 64;
pub const UNICODE_CHUNK_UNITS_CHROMIUM_MIN: usize = 32;

pub fn is_chromium_host(class: &str) -> bool {
    let class = class.trim();
    class.starts_with("Chrome_WidgetWin")
        || class.starts_with("Chrome_RenderWidgetHost")
        || class.eq_ignore_ascii_case("Chrome_RenderWidgetHostHWND")
}

pub fn chromium_uses_named_host_adapter() -> bool {
    false
}

pub fn unicode_chunk_units_for_class(class: Option<&str>) -> usize {
    if class.is_some_and(is_chromium_host) {
        UNICODE_CHUNK_UNITS_CHROMIUM
    } else {
        UNICODE_CHUNK_UNITS_DEFAULT
    }
}

pub fn next_logical_chunk_end(units: &[Vec<InsertKey>], start: usize, max_keys: usize) -> usize {
    if start >= units.len() {
        return start;
    }
    let max_keys = max_keys.clamp(1, UNICODE_CHUNK_UNITS_MAX);
    let mut keys = 0;
    let mut end = start;
    while end < units.len() {
        let n = units[end].len().max(1);
        if keys > 0 && keys + n > max_keys {
            break;
        }
        keys += n;
        end += 1;
        if keys >= max_keys {
            break;
        }
    }
    end.max(start + 1).min(units.len())
}

pub fn adapt_unicode_chunk_size(prev_chunk: usize, wall_ms: u128) -> usize {
    let min = if prev_chunk < UNICODE_CHUNK_UNITS_MIN {
        UNICODE_CHUNK_UNITS_CHROMIUM_MIN
    } else {
        UNICODE_CHUNK_UNITS_MIN
    };
    let current = prev_chunk.clamp(min, UNICODE_CHUNK_UNITS_MAX);
    if wall_ms >= 80 {
        min
    } else if wall_ms <= 8 {
        UNICODE_CHUNK_UNITS_MAX
    } else {
        current
    }
}

pub fn capture_skips_voxely_roots(
    candidate_root: usize,
    overlay_root: Option<usize>,
    main_root: Option<usize>,
) -> bool {
    overlay_root == Some(candidate_root) || main_root == Some(candidate_root)
}

pub fn resolve_insert_target(
    start: Option<NativeHwnd>,
    live: Option<NativeHwnd>,
    overlay: Option<NativeHwnd>,
    main: Option<NativeHwnd>,
) -> Option<NativeHwnd> {
    resolve_captured_insert_target(
        start.map(|hwnd| CapturedTarget {
            hwnd: hwnd.value,
            root: hwnd.value,
            pid: 0,
            tid: 0,
            generation: 0,
        }),
        live.map(|hwnd| CapturedTarget {
            hwnd: hwnd.value,
            root: hwnd.value,
            pid: 0,
            tid: 0,
            generation: 0,
        }),
        overlay.map(|hwnd| hwnd.value),
        main.map(|hwnd| hwnd.value),
    )
    .map(|target| NativeHwnd { value: target.hwnd })
}

pub fn resolve_captured_insert_target(
    start: Option<CapturedTarget>,
    live: Option<CapturedTarget>,
    overlay_root: Option<usize>,
    main_root: Option<usize>,
) -> Option<CapturedTarget> {
    if let Some(live) = live {
        if !capture_skips_voxely_roots(live.root, overlay_root, main_root) {
            return Some(live);
        }
    }
    start.filter(|target| !capture_skips_voxely_roots(target.root, overlay_root, main_root))
}

pub fn insert_should_abort(started_generation: u64, current_generation: u64) -> bool {
    started_generation != current_generation
}

pub fn hotkey_wait_complete(elapsed_ms: u32, still_down: bool, timeout_ms: u32) -> bool {
    !still_down || elapsed_ms >= timeout_ms
}

pub fn hotkey_keys_to_release(spec: &str) -> Vec<u16> {
    let mut keys = Vec::new();
    for token in spec.split('+') {
        match token.trim() {
            t if t.eq_ignore_ascii_case("Ctrl") || t.eq_ignore_ascii_case("Control") => {
                keys.push(0x11);
            }
            t if t.eq_ignore_ascii_case("Shift") => keys.push(0x10),
            t if t.eq_ignore_ascii_case("Alt") => keys.push(0x12),
            t if t.eq_ignore_ascii_case("Win") || t.eq_ignore_ascii_case("Meta") => {
                keys.push(0x5B);
            }
            t if t.eq_ignore_ascii_case("Space") => keys.push(0x20),
            t if t.eq_ignore_ascii_case("Left") => keys.push(0x25),
            t if t.eq_ignore_ascii_case("Up") => keys.push(0x26),
            t if t.eq_ignore_ascii_case("Right") => keys.push(0x27),
            t if t.eq_ignore_ascii_case("Down") => keys.push(0x28),
            _ => {}
        }
    }
    keys
}

pub fn insertion_mode_queues_keys(mode: &str) -> bool {
    mode != "clipboard"
}

pub fn run_insert(world: &mut dyn InsertWorld, req: InsertRequest<'_>) -> InsertOutcome {
    let _guard = INSERT_LOCK.lock().unwrap_or_else(|err| err.into_inner());
    run_insert_locked(world, req)
}

fn run_insert_locked(world: &mut dyn InsertWorld, req: InsertRequest<'_>) -> InsertOutcome {
    if (req.abort)() {
        return InsertOutcome::CancelledBeforeDelivery;
    }
    if req.mode == "clipboard" {
        return copy_full(world, req.text, CopyReason::ClipboardMode);
    }
    if req.text.is_empty() {
        return InsertOutcome::Inserted;
    }
    let snap = annotated_snapshot(world, req.captured, req.overlay_root, req.main_root);
    if (req.abort)() {
        return InsertOutcome::CancelledBeforeDelivery;
    }
    match classify_insert_policy(&snap) {
        InsertPolicy::CopyOnly(reason) => copy_full(world, req.text, reason),
        InsertPolicy::RestoreThenSend => restore_then_send(world, req, snap),
        InsertPolicy::SendUnicode => send_unicode(world, req, snap),
    }
}

fn annotated_snapshot(
    world: &dyn InsertWorld,
    captured: Option<CapturedTarget>,
    overlay_root: Option<usize>,
    main_root: Option<usize>,
) -> WorldSnapshot {
    let mut snap = world.snapshot(captured);
    snap.overlay_root = overlay_root;
    snap.main_root = main_root;
    snap.captured = captured;
    snap
}

fn restore_then_send(
    world: &mut dyn InsertWorld,
    req: InsertRequest<'_>,
    snap: WorldSnapshot,
) -> InsertOutcome {
    let Some(target) = snap.captured else {
        return copy_full(world, req.text, CopyReason::NoTarget);
    };
    if !world.restore_foreground(&target) {
        return copy_full(world, req.text, CopyReason::UserSwitched);
    }
    let _ = world.focus_caret(&target);
    let snap = annotated_snapshot(world, req.captured, req.overlay_root, req.main_root);
    match classify_insert_policy(&snap) {
        InsertPolicy::SendUnicode => send_unicode(world, req, snap),
        InsertPolicy::CopyOnly(reason) => copy_full(world, req.text, reason),
        InsertPolicy::RestoreThenSend => copy_full(world, req.text, CopyReason::UserSwitched),
    }
}

fn send_unicode(
    world: &mut dyn InsertWorld,
    req: InsertRequest<'_>,
    snap: WorldSnapshot,
) -> InsertOutcome {
    if let Some(target) = snap.captured {
        let _ = world.focus_caret(&target);
    }
    world.wait_keys_up(
        &hotkey_keys_to_release(req.hotkey),
        Duration::from_millis(300),
    );
    if (req.abort)() {
        return InsertOutcome::CancelledBeforeDelivery;
    }
    let units = plan_logical_units(req.text, snap.window_class.as_deref());
    let mut cursor = 0usize;
    let mut any = false;
    let mut chunk = unicode_chunk_units_for_class(snap.window_class.as_deref());
    let started_all = std::time::Instant::now();
    while cursor < units.len() {
        if (req.abort)() {
            return if any {
                copy_and(world, req.text, InsertOutcome::PartialCopied)
            } else {
                InsertOutcome::CancelledBeforeDelivery
            };
        }
        let live = annotated_snapshot(world, req.captured, req.overlay_root, req.main_root);
        if !matches!(classify_insert_policy(&live), InsertPolicy::SendUnicode) {
            return if any {
                copy_and(world, req.text, InsertOutcome::PartialCopied)
            } else {
                copy_full(world, req.text, CopyReason::UserSwitched)
            };
        }
        let end = next_logical_chunk_end(&units, cursor, chunk);
        let keys: Vec<InsertKey> = units[cursor..end].iter().flatten().copied().collect();
        let planned_events = (keys.len() * 2) as u32;
        let started = std::time::Instant::now();
        match world.send_keys(&keys) {
            ChunkSend::Complete => {
                any = true;
                cursor = end;
                chunk = adapt_unicode_chunk_size(chunk, started.elapsed().as_millis());
            }
            ChunkSend::Zero if !any => {
                return copy_full(world, req.text, CopyReason::SendBlocked);
            }
            ChunkSend::Zero | ChunkSend::Partial { .. } | ChunkSend::Odd { .. } => {
                return copy_and(world, req.text, InsertOutcome::PartialCopied);
            }
        }
        let _ = planned_events;
    }
    tracing::info!(
        policy = "send_unicode",
        units = units.len() as u64,
        insert_ms = started_all.elapsed().as_millis() as u64,
        class = snap.window_class.as_deref().unwrap_or(""),
        "unicode insert completed"
    );
    InsertOutcome::Inserted
}

fn copy_full(world: &mut dyn InsertWorld, text: &str, reason: CopyReason) -> InsertOutcome {
    match world.clipboard_copy(text) {
        Ok(()) => InsertOutcome::Copied { reason },
        Err(_) => InsertOutcome::Failed,
    }
}

fn copy_and(world: &mut dyn InsertWorld, text: &str, outcome: InsertOutcome) -> InsertOutcome {
    match world.clipboard_copy(text) {
        Ok(()) => outcome,
        Err(_) => InsertOutcome::Failed,
    }
}

pub fn insert_outcome_event(outcome: InsertOutcome) -> Option<&'static str> {
    match outcome {
        InsertOutcome::Inserted | InsertOutcome::CancelledBeforeDelivery => None,
        InsertOutcome::Copied { .. } => Some("copied"),
        InsertOutcome::PartialCopied => Some("partial"),
        InsertOutcome::Failed => Some("TextInsertionFailed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeWorld {
        captured: Option<CapturedTarget>,
        foreground_root: Option<usize>,
        overlay_root: Option<usize>,
        main_root: Option<usize>,
        foreground_pid: Option<u32>,
        alive: bool,
        iconic: bool,
        integrity_blocked: Option<bool>,
        window_class: Option<String>,
        send_script: Vec<ChunkSend>,
        send_at: usize,
        clipboard: Option<String>,
        clipboard_fail: bool,
        actions: RefCell<Vec<String>>,
        restore_ok: bool,
    }

    impl FakeWorld {
        fn target(root: usize) -> CapturedTarget {
            CapturedTarget {
                hwnd: root + 1,
                root,
                pid: 10,
                tid: 11,
                generation: 1,
            }
        }
    }

    impl InsertWorld for FakeWorld {
        fn snapshot(&self, captured: Option<CapturedTarget>) -> WorldSnapshot {
            WorldSnapshot {
                captured,
                foreground_root: self.foreground_root,
                foreground_pid: self.foreground_pid,
                focus_root: self.foreground_root,
                overlay_root: self.overlay_root,
                main_root: self.main_root,
                target_alive: self.alive,
                target_iconic: self.iconic,
                integrity_blocked: self.integrity_blocked,
                window_class: self.window_class.clone(),
            }
        }

        fn restore_foreground(&mut self, _target: &CapturedTarget) -> bool {
            self.actions.borrow_mut().push("restore".into());
            if self.restore_ok {
                if let Some(target) = self.captured {
                    self.foreground_root = Some(target.root);
                    self.foreground_pid = Some(target.pid);
                }
            }
            self.restore_ok
        }

        fn focus_caret(&mut self, _target: &CapturedTarget) -> bool {
            self.actions.borrow_mut().push("focus".into());
            true
        }

        fn wait_keys_up(&mut self, _keys: &[u16], _timeout: Duration) {
            self.actions.borrow_mut().push("wait_keys".into());
        }

        fn send_keys(&mut self, keys: &[InsertKey]) -> ChunkSend {
            self.actions
                .borrow_mut()
                .push(format!("send:{}", keys.len()));
            let result = self
                .send_script
                .get(self.send_at)
                .copied()
                .unwrap_or(ChunkSend::Complete);
            self.send_at += 1;
            result
        }

        fn clipboard_copy(&mut self, text: &str) -> Result<(), AppError> {
            self.actions
                .borrow_mut()
                .push(format!("copy:{}", text.len()));
            if self.clipboard_fail {
                return Err(AppError::TextInsertionFailed("clipboard busy".into()));
            }
            self.clipboard = Some(text.to_string());
            Ok(())
        }
    }

    fn request<'a>(
        mode: &'a str,
        text: &'a str,
        captured: Option<CapturedTarget>,
        abort: &'a dyn Fn() -> bool,
    ) -> InsertRequest<'a> {
        InsertRequest {
            mode,
            text,
            captured,
            overlay_root: Some(3),
            main_root: Some(4),
            hotkey: "Ctrl+Shift+Q",
            abort,
        }
    }

    #[test]
    fn captured_foreground_sends_without_restore() {
        let target = FakeWorld::target(10);
        let mut world = FakeWorld {
            captured: Some(target),
            foreground_root: Some(10),
            overlay_root: Some(3),
            main_root: Some(4),
            alive: true,
            restore_ok: false,
            ..FakeWorld::default()
        };
        let never = || false;
        let outcome = run_insert(&mut world, request("unicode", "hi", Some(target), &never));
        assert_eq!(outcome, InsertOutcome::Inserted);
        assert_eq!(
            world.actions.borrow().as_slice(),
            ["focus", "wait_keys", "send:2"]
        );
        assert!(world.clipboard.is_none());
    }

    #[test]
    fn voxely_foreground_restores_once() {
        let target = FakeWorld::target(10);
        let mut world = FakeWorld {
            captured: Some(target),
            foreground_root: Some(3),
            overlay_root: Some(3),
            main_root: Some(4),
            alive: true,
            restore_ok: true,
            ..FakeWorld::default()
        };
        let never = || false;
        let outcome = run_insert(&mut world, request("unicode", "hi", Some(target), &never));
        assert_eq!(outcome, InsertOutcome::Inserted);
        assert_eq!(
            world.actions.borrow().as_slice(),
            ["restore", "focus", "focus", "wait_keys", "send:2"]
        );
    }

    #[test]
    fn third_party_foreground_never_restores() {
        let target = FakeWorld::target(10);
        let mut world = FakeWorld {
            captured: Some(target),
            foreground_root: Some(99),
            foreground_pid: Some(99),
            overlay_root: Some(3),
            main_root: Some(4),
            alive: true,
            restore_ok: true,
            ..FakeWorld::default()
        };
        let never = || false;
        let outcome = run_insert(
            &mut world,
            request("unicode", "full text", Some(target), &never),
        );
        assert_eq!(
            outcome,
            InsertOutcome::Copied {
                reason: CopyReason::UserSwitched
            }
        );
        assert_eq!(world.actions.borrow().as_slice(), ["copy:9"]);
        assert_eq!(world.clipboard.as_deref(), Some("full text"));
    }

    #[test]
    fn live_focus_wins_over_start_capture() {
        let start = FakeWorld::target(10);
        let live = CapturedTarget {
            hwnd: 21,
            root: 20,
            pid: 20,
            tid: 21,
            generation: 1,
        };
        assert_eq!(
            resolve_captured_insert_target(Some(start), Some(live), Some(3), Some(4)),
            Some(live)
        );
        assert_eq!(
            resolve_captured_insert_target(
                Some(start),
                Some(CapturedTarget {
                    hwnd: 3,
                    root: 3,
                    pid: 1,
                    tid: 1,
                    generation: 1,
                }),
                Some(3),
                Some(4)
            ),
            Some(start)
        );
        assert_eq!(
            resolve_captured_insert_target(None, Some(live), Some(3), Some(4)),
            Some(live)
        );
        assert_eq!(
            resolve_insert_target(
                Some(NativeHwnd { value: 10 }),
                Some(NativeHwnd { value: 20 }),
                Some(NativeHwnd { value: 3 }),
                Some(NativeHwnd { value: 4 }),
            ),
            Some(NativeHwnd { value: 20 })
        );
    }

    #[test]
    fn same_process_other_hwnd_restores_then_sends() {
        let target = FakeWorld::target(10);
        let mut world = FakeWorld {
            captured: Some(target),
            foreground_root: Some(77),
            foreground_pid: Some(10),
            overlay_root: Some(3),
            main_root: Some(4),
            alive: true,
            restore_ok: true,
            ..FakeWorld::default()
        };
        let never = || false;
        let outcome = run_insert(&mut world, request("unicode", "hi", Some(target), &never));
        assert_eq!(outcome, InsertOutcome::Inserted);
        assert_eq!(
            world.actions.borrow().as_slice(),
            ["restore", "focus", "focus", "wait_keys", "send:2"]
        );
    }

    #[test]
    fn chromium_chunk_stays_small_when_slow() {
        assert!(is_chromium_host("Chrome_WidgetWin_1"));
        assert!(is_chromium_host("Chrome_RenderWidgetHostHWND"));
        assert!(!is_chromium_host("Edit"));
        assert!(!chromium_uses_named_host_adapter());
        assert_eq!(
            unicode_chunk_units_for_class(Some("Chrome_WidgetWin_1")),
            UNICODE_CHUNK_UNITS_CHROMIUM
        );
        assert_eq!(
            adapt_unicode_chunk_size(64, 400),
            UNICODE_CHUNK_UNITS_CHROMIUM_MIN
        );
    }

    #[test]
    fn missing_and_minimized_targets_copy() {
        let never = || false;
        let mut missing = FakeWorld {
            alive: false,
            ..FakeWorld::default()
        };
        assert_eq!(
            run_insert(&mut missing, request("unicode", "hi", None, &never)),
            InsertOutcome::Copied {
                reason: CopyReason::NoTarget
            }
        );
        let target = FakeWorld::target(10);
        let mut minimized = FakeWorld {
            captured: Some(target),
            foreground_root: Some(10),
            alive: true,
            iconic: true,
            ..FakeWorld::default()
        };
        assert_eq!(
            run_insert(
                &mut minimized,
                request("unicode", "hi", Some(target), &never)
            ),
            InsertOutcome::Copied {
                reason: CopyReason::Minimized
            }
        );
        assert!(!minimized.actions.borrow().iter().any(|a| a == "restore"));
    }

    #[test]
    fn zero_delivery_copies_full_without_retry() {
        let target = FakeWorld::target(10);
        let mut world = FakeWorld {
            captured: Some(target),
            foreground_root: Some(10),
            alive: true,
            send_script: vec![ChunkSend::Zero, ChunkSend::Complete],
            ..FakeWorld::default()
        };
        let never = || false;
        let outcome = run_insert(&mut world, request("unicode", "abc", Some(target), &never));
        assert_eq!(
            outcome,
            InsertOutcome::Copied {
                reason: CopyReason::SendBlocked
            }
        );
        assert_eq!(world.clipboard.as_deref(), Some("abc"));
        assert_eq!(
            world
                .actions
                .borrow()
                .iter()
                .filter(|a| a.starts_with("send"))
                .count(),
            1
        );
    }

    #[test]
    fn even_partial_does_not_replay() {
        let target = FakeWorld::target(10);
        let mut world = FakeWorld {
            captured: Some(target),
            foreground_root: Some(10),
            alive: true,
            send_script: vec![ChunkSend::Partial { accepted: 4 }, ChunkSend::Complete],
            ..FakeWorld::default()
        };
        let never = || false;
        let outcome = run_insert(
            &mut world,
            request("unicode", "abcdef", Some(target), &never),
        );
        assert_eq!(outcome, InsertOutcome::PartialCopied);
        assert_eq!(world.clipboard.as_deref(), Some("abcdef"));
        assert_eq!(
            world
                .actions
                .borrow()
                .iter()
                .filter(|a| a.starts_with("send"))
                .count(),
            1
        );
    }

    #[test]
    fn odd_partial_copies_full() {
        let target = FakeWorld::target(10);
        let mut world = FakeWorld {
            captured: Some(target),
            foreground_root: Some(10),
            alive: true,
            send_script: vec![ChunkSend::Odd { accepted: 3 }],
            ..FakeWorld::default()
        };
        let never = || false;
        assert_eq!(
            run_insert(&mut world, request("unicode", "ab", Some(target), &never)),
            InsertOutcome::PartialCopied
        );
    }

    #[test]
    fn cancel_before_delivery_does_not_copy() {
        let target = FakeWorld::target(10);
        let mut world = FakeWorld {
            captured: Some(target),
            foreground_root: Some(10),
            alive: true,
            ..FakeWorld::default()
        };
        let abort = || true;
        assert_eq!(
            run_insert(&mut world, request("unicode", "hi", Some(target), &abort)),
            InsertOutcome::CancelledBeforeDelivery
        );
        assert!(world.clipboard.is_none());
    }

    #[test]
    fn cancel_after_possible_delivery_is_partial_copied() {
        let target = FakeWorld::target(10);
        let text = "a".repeat(600);
        let mut world = FakeWorld {
            captured: Some(target),
            foreground_root: Some(10),
            alive: true,
            send_script: vec![ChunkSend::Complete],
            ..FakeWorld::default()
        };
        let count = std::cell::Cell::new(0u32);
        let abort = || {
            let n = count.get();
            count.set(n + 1);
            n >= 4
        };
        let outcome = run_insert(
            &mut world,
            InsertRequest {
                mode: "unicode",
                text: &text,
                captured: Some(target),
                overlay_root: Some(3),
                main_root: Some(4),
                hotkey: "Ctrl+Shift+Q",
                abort: &abort,
            },
        );
        assert_eq!(outcome, InsertOutcome::PartialCopied);
        assert_eq!(world.clipboard.as_deref(), Some(text.as_str()));
        assert!(world.actions.borrow().iter().any(|a| a.starts_with("send")));
    }

    #[test]
    fn clipboard_busy_is_failed() {
        let mut world = FakeWorld {
            clipboard_fail: true,
            ..FakeWorld::default()
        };
        let never = || false;
        assert_eq!(
            run_insert(&mut world, request("clipboard", "hi", None, &never)),
            InsertOutcome::Failed
        );
    }

    #[test]
    fn clipboard_mode_copies_without_send() {
        let target = FakeWorld::target(10);
        let mut world = FakeWorld {
            captured: Some(target),
            foreground_root: Some(10),
            alive: true,
            ..FakeWorld::default()
        };
        let never = || false;
        assert_eq!(
            run_insert(
                &mut world,
                request("clipboard", "paste me", Some(target), &never)
            ),
            InsertOutcome::Copied {
                reason: CopyReason::ClipboardMode
            }
        );
        assert!(!world.actions.borrow().iter().any(|a| a.starts_with("send")));
        assert!(!insertion_mode_queues_keys("clipboard"));
    }

    #[test]
    fn integrity_blocked_copies() {
        let target = FakeWorld::target(10);
        let mut world = FakeWorld {
            captured: Some(target),
            foreground_root: Some(10),
            alive: true,
            integrity_blocked: Some(true),
            ..FakeWorld::default()
        };
        let never = || false;
        assert_eq!(
            run_insert(&mut world, request("unicode", "hi", Some(target), &never)),
            InsertOutcome::Copied {
                reason: CopyReason::IntegrityBlocked
            }
        );
    }

    #[test]
    fn surrogate_is_one_logical_unit_on_chunk_boundary() {
        let bmp = "a".repeat(511);
        let text = format!("{bmp}👍");
        let units = plan_logical_units(&text, None);
        assert_eq!(units.len(), 512);
        assert_eq!(units[511].len(), 2);
        let end = next_logical_chunk_end(&units, 0, 512);
        assert_eq!(end, 511);
        assert_eq!(next_logical_chunk_end(&units, end, 512), 512);
        let split = next_logical_chunk_end(&units, 0, 511);
        assert_eq!(split, 511);
        assert_eq!(units[split].len(), 2);
        assert_eq!(classify_chunk_send(0, 4), ChunkSend::Zero);
        assert_eq!(classify_chunk_send(3, 4), ChunkSend::Odd { accepted: 3 });
        assert_eq!(
            classify_chunk_send(2, 4),
            ChunkSend::Partial { accepted: 2 }
        );
    }

    #[test]
    fn crlf_and_edit_class_plan_one_unit() {
        assert_eq!(
            plan_logical_units("a\r\nb", None),
            vec![
                vec![InsertKey::Unicode(b'a' as u16)],
                vec![InsertKey::Unicode(0x0A)],
                vec![InsertKey::Unicode(b'b' as u16)],
            ]
        );
        assert_eq!(
            plan_insert_units_for_class("a\r\nb", Some("Edit")),
            vec![
                InsertKey::Unicode(b'a' as u16),
                InsertKey::VirtualKey(0x0D),
                InsertKey::Unicode(b'b' as u16),
            ]
        );
        assert!(edit_class_uses_vk_return("RICHEDIT50W"));
    }

    #[test]
    fn generation_abort_helper() {
        assert!(!insert_should_abort(3, 3));
        assert!(insert_should_abort(3, 4));
    }
}
