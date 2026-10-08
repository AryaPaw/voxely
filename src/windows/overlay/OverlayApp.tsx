import { useEffect, useLayoutEffect, useRef, useState, type PointerEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { api, type AppSettings, type OverlaySnapshot, type SessionState } from "../../lib/api";
import { applyUiLocale, messagesFor, resolveUiLocale } from "../../lib/i18n";
import { overlayHudLabel, overlayIsBusy, overlayShouldRender } from "../../lib/session-copy";
import { acceptOverlayRevision, applyOverlaySnapshot } from "../../lib/overlay-snapshot";
import { overlayCancelArmed } from "../../lib/overlay-wave";
import { bindEscapeCancel } from "../../lib/escape-cancel";
import { applyTheme, watchSystemTheme } from "../../lib/theme";
import { OverlayWave } from "./OverlayWave";

const HIDDEN: OverlaySnapshot = {
  revision: 0,
  visible: false,
  state: { kind: "idle" },
};

export function OverlayApp() {
  const [snapshot, setSnapshot] = useState<OverlaySnapshot>(HIDDEN);
  const [elapsed, setElapsed] = useState(0);
  const [hovered, setCancelHover] = useState(false);
  const hoverEpoch = useRef(0);
  const hoverCheckInFlight = useRef(false);
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
    return bindEscapeCancel(() => {
      void api.cancel().catch(() => undefined);
    });
  }, []);

  useLayoutEffect(() => {
    hoverEpoch.current += 1;
    hoverCheckInFlight.current = false;
    setCancelHover(false);
  }, [busy, snapshot.visible]);

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
      hoverEpoch.current += 1;
      hoverCheckInFlight.current = false;
      setCancelHover(false);
      void api.cancel().catch(() => undefined);
    }
  }

  function checkCancelHover(event: PointerEvent<HTMLButtonElement>) {
    if (!busy || !snapshot.visible || hovered || hoverCheckInFlight.current) {
      return;
    }

    const epoch = hoverEpoch.current;
    hoverCheckInFlight.current = true;
    void api
      .overlayPointerMatches(event.clientX, event.clientY)
      .then((matches) => {
        if (epoch === hoverEpoch.current && busy && snapshot.visible) {
          setCancelHover(matches);
        }
      })
      .catch(() => undefined)
      .finally(() => {
        if (epoch === hoverEpoch.current) {
          hoverCheckInFlight.current = false;
        }
      });
  }

  function disarmCancelHover() {
    hoverEpoch.current += 1;
    hoverCheckInFlight.current = false;
    setCancelHover(false);
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
        type="button"
        className={`overlay-pill${cancelReady ? " overlay-pill-cancel" : ""}${busy && !cancelReady ? " overlay-pill-busy" : ""}`}
        onClick={onPillClick}
        onPointerEnter={checkCancelHover}
        onPointerMove={checkCancelHover}
        onPointerLeave={disarmCancelHover}
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
