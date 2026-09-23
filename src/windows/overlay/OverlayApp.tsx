import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { api, type AppSettings, type OverlaySnapshot, type SessionState } from "../../lib/api";
import { applyUiLocale, messagesFor, resolveUiLocale } from "../../lib/i18n";
import { overlayHudLabel, overlayIsBusy, overlayShouldRender } from "../../lib/session-copy";
import { acceptOverlayRevision, applyOverlaySnapshot } from "../../lib/overlay-snapshot";
import {
  overlayCancelArmed,
  overlayHoverFromElement,
  overlayHoverFromPoll,
} from "../../lib/overlay-wave";
import { applyTheme, watchSystemTheme } from "../../lib/theme";
import { OverlayWave } from "./OverlayWave";

const HIDDEN: OverlaySnapshot = {
  revision: 0,
  visible: false,
  state: { kind: "idle" },
};

export function OverlayApp() {
  const pillRef = useRef<HTMLButtonElement>(null);
  const [snapshot, setSnapshot] = useState<OverlaySnapshot>(HIDDEN);
  const [elapsed, setElapsed] = useState(0);
  const [hovered, setCancelHover] = useState(false);
  const [copy, setCopy] = useState(() => messagesFor("ru"));
  const [theme, setTheme] = useState("dark");
  const state: SessionState = snapshot.state;
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
    void invoke("overlay_mark_frame", { phase: "react" }).catch(() => undefined);
    const frame = window.requestAnimationFrame(() => {
      void invoke("overlay_mark_frame", { phase: "frame" }).catch(() => undefined);
    });
    void api
      .settings()
      .then(applySettings)
      .catch(() => undefined);
    let revision = 0;
    let unlistenFn: (() => void) | undefined;
    let cancelled = false;
    const unlistenState = listen<OverlaySnapshot>("overlay://snapshot", (event) => {
      if (!acceptOverlayRevision(revision, event.payload.revision)) {
        return;
      }
      revision = event.payload.revision;
      setSnapshot((current) => applyOverlaySnapshot(current, event.payload));
    }).then(async (fn) => {
      if (cancelled) {
        fn();
        return;
      }
      unlistenFn = fn;
      const initial = await api.overlaySnapshot();
      if (cancelled) {
        return;
      }
      if (acceptOverlayRevision(revision, initial.revision)) {
        revision = initial.revision;
        setSnapshot(initial);
      }
    });
    const unlistenSettings = listen<AppSettings>("settings://changed", (event) =>
      applySettings(event.payload),
    );
    return () => {
      cancelled = true;
      window.cancelAnimationFrame(frame);
      unlistenFn?.();
      void unlistenState;
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
      setCancelHover((previous) =>
        overlayHoverFromPoll(previous, overlayHoverFromElement(pillRef.current)),
      );
    };
    const disarm = () => setCancelHover(false);
    sync();
    let frame = 0;
    const tick = () => {
      sync();
      frame = window.requestAnimationFrame(tick);
    };
    if (import.meta.env.MODE !== "test") {
      frame = window.requestAnimationFrame(tick);
    }
    window.addEventListener("pointerleave", disarm);
    window.addEventListener("blur", disarm);
    return () => {
      window.cancelAnimationFrame(frame);
      window.removeEventListener("pointerleave", disarm);
      window.removeEventListener("blur", disarm);
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

  function onPillClick() {
    if (cancelReady) {
      void api.cancel().catch(() => undefined);
    }
  }

  const status = overlayHudLabel(snapshot.visible, state, copy, Boolean(snapshot.limitReached));
  const showDots = busy && !cancelReady && !recording;
  const clock = recording && !cancelReady ? formatClock(elapsed) : "";
  const cancelHint = cancelReady ? copy.overlayCancel : busy ? copy.overlayCancelAria : status;

  if (!overlayShouldRender(snapshot.visible, state)) {
    return <div className="overlay-shell overlay-shell-hidden" aria-hidden="true" />;
  }

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
