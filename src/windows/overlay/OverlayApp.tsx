import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type AppSettings, type SessionState } from "../../lib/api";
import { applyUiLocale, messagesFor, resolveUiLocale } from "../../lib/i18n";
import { overlayIsBusy, overlayLabel } from "../../lib/session-copy";
import { overlayCancelArmed, overlayHoverFromElement } from "../../lib/overlay-wave";
import { applyTheme, watchSystemTheme } from "../../lib/theme";
import { OverlayWave } from "./OverlayWave";

export function OverlayApp() {
  const pillRef = useRef<HTMLButtonElement>(null);
  const [state, setState] = useState<SessionState>({ kind: "idle" });
  const [elapsed, setElapsed] = useState(0);
  const [hovered, setCancelHover] = useState(false);
  const [copy, setCopy] = useState(() => messagesFor("ru"));
  const [theme, setTheme] = useState("dark");
  const recording = state.kind === "recording" || state.kind === "startingRecording";
  const busy = overlayIsBusy(state);
  const cancelReady = overlayCancelArmed(busy, hovered);

  function applySettings(settings: AppSettings) {
    setTheme(settings.theme);
    applyTheme(settings.theme);
    const locale = resolveUiLocale(settings.uiLanguage ?? "auto", navigator.language);
    applyUiLocale(locale);
    setCopy(messagesFor(locale));
  }

  useEffect(() => {
    void api.settings().then(applySettings);
    void api.session().then(setState);
    const unlistenState = listen<SessionState>("session://state", (event) =>
      setState(event.payload),
    );
    const unlistenSettings = listen<AppSettings>("settings://changed", (event) =>
      applySettings(event.payload),
    );
    return () => {
      void unlistenState.then((fn) => fn());
      void unlistenSettings.then((fn) => fn());
    };
  }, []);

  useEffect(() => watchSystemTheme(theme), [theme]);

  useEffect(() => {
    if (!busy) {
      setCancelHover(false);
    }
  }, [busy]);

  useEffect(() => {
    if (!busy) {
      return;
    }
    const sync = () => {
      if (overlayHoverFromElement(pillRef.current)) {
        setCancelHover(true);
      }
    };
    sync();
    const frame = window.requestAnimationFrame(sync);
    const timer = window.setTimeout(sync, 40);
    window.addEventListener("pointermove", sync);
    return () => {
      window.cancelAnimationFrame(frame);
      window.clearTimeout(timer);
      window.removeEventListener("pointermove", sync);
    };
  }, [busy]);

  useEffect(() => {
    if (recording) {
      setElapsed(0);
    }
  }, [recording]);

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
    if (cancelReady) {
      void api.cancel();
    }
  }

  const status = overlayLabel(state, copy);
  const showDots = busy && !cancelReady && !recording;
  const clock = recording && !cancelReady ? formatClock(elapsed) : "";
  const cancelHint = cancelReady ? copy.overlayCancel : busy ? copy.overlayCancelAria : status;

  return (
    <div className="overlay-shell">
      <button
        ref={pillRef}
        type="button"
        className={`overlay-pill${cancelReady ? " overlay-pill-cancel" : ""}${busy && !cancelReady ? " overlay-pill-busy" : ""}`}
        onClick={onPillClick}
        onMouseEnter={() => {
          if (busy) {
            setCancelHover(true);
          }
        }}
        onMouseLeave={() => setCancelHover(false)}
        aria-label={cancelHint}
      >
        {cancelReady ? (
          <span className="overlay-center overlay-cancel">{copy.overlayCancel}</span>
        ) : (
          <>
            <span className={`overlay-dot${recording ? " overlay-dot-live" : ""}`} />
            {busy && !showDots ? <OverlayWave active={recording} /> : null}
            {showDots ? (
              <span className="overlay-center">
                {status}
                <AnimatedDots />
              </span>
            ) : (
              <div className="overlay-meta">
                <span className="overlay-status">{status}</span>
                {clock ? <span className="overlay-hint">{clock}</span> : null}
              </div>
            )}
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

function formatClock(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}
