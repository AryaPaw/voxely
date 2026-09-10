import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type SessionState } from "../../lib/api";
import { playDictationCue } from "../../lib/overlay-cue";
import { drawOverlayWave, overlayBarHeights } from "../../lib/overlay-wave";
import { overlayIsBusy, overlayLabel } from "../../lib/session-copy";
import { applyTheme } from "../../lib/theme";

export function OverlayApp() {
  const [state, setState] = useState<SessionState>({ kind: "idle" });
  const [elapsed, setElapsed] = useState(0);
  const [hovered, setCancelHover] = useState(false);
  const [notify, setNotify] = useState(true);
  const recording = state.kind === "recording" || state.kind === "startingRecording";
  const busy = overlayIsBusy(state);
  const cancelReady = recording && hovered;

  useEffect(() => {
    void api.settings().then((settings) => {
      applyTheme(settings.theme);
      setNotify(settings.notifications);
    });
    void api.session().then(setState);
    const unlisten = listen<SessionState>("session://state", (event) => setState(event.payload));
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (!recording) {
      setCancelHover(false);
    }
  }, [recording]);

  const wasRecording = useRef(false);
  useEffect(() => {
    if (recording && !wasRecording.current) {
      setElapsed(0);
      if (notify) {
        playDictationCue("start");
      }
    }
    if (!recording && wasRecording.current && notify) {
      playDictationCue("stop");
    }
    wasRecording.current = recording;
  }, [notify, recording]);

  useEffect(() => {
    if (!recording) {
      return;
    }
    const clock = window.setInterval(() => setElapsed((value) => value + 1), 1000);
    return () => window.clearInterval(clock);
  }, [recording]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        void api.cancel();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  function onPillClick() {
    if (recording) {
      void api.cancel();
    }
  }

  const centered = cancelReady || (busy && !recording);
  const status = cancelReady ? "Отменить запись" : overlayLabel(state);
  const showDots = busy && !cancelReady;
  const clock = recording && !cancelReady ? formatClock(elapsed) : "";

  return (
    <div className="overlay-shell">
      <button
        type="button"
        className={`overlay-pill${cancelReady ? " overlay-pill-armed" : ""}${busy && !cancelReady ? " overlay-pill-busy" : ""}`}
        onClick={onPillClick}
        onMouseEnter={() => {
          if (recording) {
            setCancelHover(true);
          }
        }}
        onMouseLeave={() => setCancelHover(false)}
        aria-label={recording ? "Наведите, чтобы отменить запись" : status}
      >
        {centered ? (
          <span className="overlay-center">
            {status}
            {showDots ? <AnimatedDots /> : null}
          </span>
        ) : (
          <>
            <span className={`overlay-dot${recording ? " overlay-dot-live" : ""}`} />
            <OverlayWave active={recording} />
            <div className="overlay-meta">
              <span className="overlay-status">
                {status}
                {showDots ? <AnimatedDots /> : null}
              </span>
              {clock ? <span className="overlay-hint">{clock}</span> : null}
            </div>
          </>
        )}
      </button>
    </div>
  );
}

function AnimatedDots() {
  return (
    <span className="overlay-ellipsis" aria-hidden="true">
      <span>.</span>
      <span>.</span>
      <span>.</span>
    </span>
  );
}

function OverlayWave({ active }: { active: boolean }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const levelsRef = useRef<number[]>([]);
  const barsRef = useRef<number[]>([]);

  useEffect(() => {
    if (!active) {
      levelsRef.current = [];
      barsRef.current = [];
      return;
    }
    const timer = window.setInterval(() => {
      void api.meter().then((sample) => {
        levelsRef.current = sample.levels ?? [];
      });
    }, 50);
    return () => window.clearInterval(timer);
  }, [active]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) {
      return;
    }
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      return;
    }
    let frame = 0;
    const draw = () => {
      const dpr = window.devicePixelRatio || 1;
      const width = canvas.clientWidth;
      const height = canvas.clientHeight;
      if (canvas.width !== Math.floor(width * dpr) || canvas.height !== Math.floor(height * dpr)) {
        canvas.width = Math.floor(width * dpr);
        canvas.height = Math.floor(height * dpr);
      }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      const next = overlayBarHeights(levelsRef.current, active, barsRef.current);
      barsRef.current = next;
      drawOverlayWave(ctx, width, height, active ? next : []);
      frame = window.requestAnimationFrame(draw);
    };
    frame = window.requestAnimationFrame(draw);
    return () => window.cancelAnimationFrame(frame);
  }, [active]);

  return <canvas ref={canvasRef} className="overlay-wave" aria-hidden="true" />;
}

function formatClock(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}
