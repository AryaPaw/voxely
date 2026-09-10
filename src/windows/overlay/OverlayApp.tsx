import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type AppSettings, type SessionState } from "../../lib/api";
import { applyUiLocale, messagesFor, resolveUiLocale } from "../../lib/i18n";
import { overlayIsBusy, overlayLabel } from "../../lib/session-copy";
import { applyTheme, watchSystemTheme } from "../../lib/theme";
import { OverlayWave } from "./OverlayWave";

export function OverlayApp() {
  const [state, setState] = useState<SessionState>({ kind: "idle" });
  const [elapsed, setElapsed] = useState(0);
  const [hovered, setCancelHover] = useState(false);
  const [copy, setCopy] = useState(() => messagesFor("ru"));
  const [theme, setTheme] = useState("dark");
  const recording = state.kind === "recording" || state.kind === "startingRecording";
  const busy = overlayIsBusy(state);
  const cancelReady = busy && hovered;

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
    if (busy) {
      void api.cancel();
    }
  }

  const centered = cancelReady || (busy && !recording);
  const status = cancelReady ? copy.overlayCancel : overlayLabel(state, copy);
  const showDots = busy && !cancelReady;
  const clock = recording && !cancelReady ? formatClock(elapsed) : "";
  const cancelHint = busy ? copy.overlayCancel : status;

  return (
    <div className="overlay-shell">
      <button
        type="button"
        className={`overlay-pill${cancelReady ? " overlay-pill-armed" : ""}${busy && !cancelReady ? " overlay-pill-busy" : ""}`}
        onClick={onPillClick}
        onMouseEnter={() => {
          if (busy) {
            setCancelHover(true);
          }
        }}
        onMouseLeave={() => setCancelHover(false)}
        aria-label={cancelHint}
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

function formatClock(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}
