//! Explicitly authorized hardware acceptance. Never run by the default suite.
use super::*;
use std::time::Instant;

struct DiscardProbeAudio(CaptureCleanupHandle);

impl Drop for DiscardProbeAudio {
    fn drop(&mut self) {
        self.0.discard_late_result();
    }
}

struct CpuLoad {
    stop: Arc<AtomicBool>,
    workers: Vec<JoinHandle<u64>>,
}

impl CpuLoad {
    fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let count = thread::available_parallelism()
            .map_or(2, usize::from)
            .div_ceil(2)
            .clamp(1, 4);
        let workers = (0..count)
            .map(|seed| {
                let stop = stop.clone();
                thread::spawn(move || {
                    let mut value = seed as u64 + 1;
                    let mut iterations = 0u64;
                    while !stop.load(Ordering::Relaxed) {
                        for _ in 0..1024 {
                            value ^= value << 13;
                            value ^= value >> 7;
                            value ^= value << 17;
                            value = std::hint::black_box(value);
                        }
                        iterations += 1024;
                    }
                    iterations
                })
            })
            .collect();
        eprintln!("NATIVE: CPU load started, workers={count}");
        Self { stop, workers }
    }
}

impl Drop for CpuLoad {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let mut iterations = 0u64;
        for worker in self.workers.drain(..) {
            if let Ok(count) = worker.join() {
                iterations += count;
            }
        }
        eprintln!("NATIVE: CPU load stopped, work_iterations={iterations}");
    }
}

fn wait_until_stop(session: &CaptureSession, duration: Duration) -> bool {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if session.is_finished() {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
    !session.is_finished()
}

fn check_wav(result: &CaptureResult) {
    let reader = hound::WavReader::open(&result.path).unwrap();
    assert_eq!(reader.spec().sample_rate, SAMPLE_RATE);
    assert_eq!(reader.spec().channels, 1);
    assert_eq!(
        u64::from(reader.duration()) * 1000 / u64::from(SAMPLE_RATE),
        result.duration_ms
    );
    assert!(!result.path.with_extension("wav.tmp").exists());
    assert!(
        result.capture_error.is_none(),
        "native device failed: {:?}",
        result.capture_error
    );
}

struct NativeProbe {
    session: CaptureSession,
    cleanup: DiscardProbeAudio,
    input_rate: u32,
    overflow: Arc<AtomicU32>,
    continuity: Arc<CaptureContinuity>,
}

fn start_probe(device: Option<String>, path: PathBuf, writer_delay: Duration) -> NativeProbe {
    let cleanup = CaptureCleanupHandle::new(Some(path.clone()));
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let meter = Arc::new(Mutex::new(MeterSample::silent()));
    let worker_meter = meter.clone();
    let fallback = Arc::new(AtomicBool::new(false));
    let worker_fallback = fallback.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let built = start_stream_inner(device.as_deref(), worker_meter, worker_fallback)?;
        ready_tx
            .send((
                built.input_rate,
                built.overflow.clone(),
                built.continuity.clone(),
            ))
            .map_err(|_| AppError::AudioCaptureFailed("native probe controller exited".into()))?;
        if !writer_delay.is_zero() {
            thread::sleep(writer_delay);
        }
        run_capture(built, path, worker_stop)
    });
    let session = CaptureSession {
        stop,
        meter,
        thread: Some(worker),
        fallback,
        cleanup,
    };
    let cleanup = DiscardProbeAudio(session.cleanup_handle());
    let (input_rate, overflow, continuity) = ready_rx.recv_timeout(START_TIMEOUT).unwrap();
    NativeProbe {
        session,
        cleanup,
        input_rate,
        overflow,
        continuity,
    }
}

#[test]
#[ignore = "requires explicit microphone consent and VOXELY_NATIVE_CAPTURE_TEST=1"]
fn native_capture_survives_load_and_forced_overflow() {
    assert_eq!(
        std::env::var("VOXELY_NATIVE_CAPTURE_TEST").as_deref(),
        Ok("1")
    );
    let root = std::env::var_os("VOXELY_NATIVE_CAPTURE_DIR")
        .expect("provide a disposable audio directory");
    let temporary = tempfile::tempdir_in(root).unwrap();
    let device = std::env::var("VOXELY_NATIVE_CAPTURE_DEVICE").ok();
    let _load = CpuLoad::start();
    for (index, seconds) in [8, 20, 60].into_iter().enumerate() {
        let run = index + 1;
        let path = temporary.path().join(format!("normal-{run}.wav"));
        let NativeProbe {
            session,
            cleanup: _normal_cleanup,
            overflow,
            continuity,
            ..
        } = start_probe(device.clone(), path, Duration::ZERO);
        let fallback = session.used_fallback_device();
        let stayed_active = wait_until_stop(&session, Duration::from_secs(seconds));
        let result = session.stop().unwrap();
        check_wav(&result);
        assert!(!fallback, "requested device was not available");
        assert!(
            stayed_active,
            "native recording stopped without a stop request"
        );
        assert_eq!(
            overflow.load(Ordering::Relaxed),
            0,
            "ordinary CPU load overflowed the app queue"
        );
        let device_gap = continuity.device_discontinuity.load(Ordering::Relaxed);
        assert_eq!(
            result.truncated, device_gap,
            "WASAPI discontinuities must be reflected in the result"
        );
        assert!(
            result.duration_ms >= (seconds - 2) * 1000,
            "native capture lost recording time"
        );
        eprintln!(
            "NATIVE PASS: run={run}, normal_load, audio_ms={}, device_discontinuity={device_gap}, app_queue_drop=0",
            result.duration_ms
        );
        std::fs::remove_file(&result.path).unwrap();

        let path = temporary.path().join(format!("overflow-{run}.wav"));
        // Hold the writer while real microphone callbacks fill the ten-second queue.
        let NativeProbe {
            session,
            cleanup: _overflow_cleanup,
            input_rate,
            overflow,
            ..
        } = start_probe(
            device.clone(),
            path,
            Duration::from_secs(QUEUE_SECONDS as u64 + 2),
        );
        let stayed_active = wait_until_stop(&session, Duration::from_secs(18));
        let fallback = session.used_fallback_device();
        let result = session.stop().unwrap();
        let dropped = overflow.load(Ordering::Relaxed);
        check_wav(&result);
        assert!(!fallback, "requested device was not available");
        assert!(
            dropped > 0,
            "probe did not cause an actual native queue overflow"
        );
        assert!(
            stayed_active,
            "native overflow stopped recording before explicit stop"
        );
        assert!(result.truncated, "native audio loss was not marked");
        assert!(
            result.duration_ms > 12_000,
            "capture did not retain audio after queue recovery"
        );
        eprintln!("NATIVE PASS: run={run}, forced_stall_seconds=12, input_rate={input_rate}, dropped_frames={dropped}, audio_ms={}, active_until_explicit_stop=true", result.duration_ms);
        std::fs::remove_file(&result.path).unwrap();
    }
    assert_eq!(std::fs::read_dir(temporary.path()).unwrap().count(), 0);
    eprintln!("NATIVE PASS: six microphone captures completed, no audio retained, no API or history access");
}

#[test]
#[ignore = "requires explicit microphone consent and VOXELY_NATIVE_CAPTURE_TEST=1"]
fn native_wasapi_startup_and_later_discontinuities() {
    assert_eq!(
        std::env::var("VOXELY_NATIVE_CAPTURE_TEST").as_deref(),
        Ok("1")
    );
    let device = std::env::var("VOXELY_NATIVE_CAPTURE_DEVICE").ok();
    for run in 1..=3 {
        let fallback = Arc::new(AtomicBool::new(false));
        let built = start_stream_inner(
            device.as_deref(),
            Arc::new(Mutex::new(MeterSample::silent())),
            fallback.clone(),
        )
        .unwrap();
        thread::sleep(Duration::from_millis(500));
        drop(built.stream);
        assert!(!fallback.load(Ordering::Relaxed));
        assert!(built.continuity.received_packet.load(Ordering::Relaxed));
        assert!(built.stream_error.lock().is_none());
        assert_eq!(built.overflow.load(Ordering::Relaxed), 0);
        eprintln!(
            "NATIVE: run={run}, startup_discontinuity={}, device_discontinuity={}",
            built
                .continuity
                .startup_discontinuity
                .load(Ordering::Relaxed),
            built
                .continuity
                .device_discontinuity
                .load(Ordering::Relaxed),
        );
    }
}
