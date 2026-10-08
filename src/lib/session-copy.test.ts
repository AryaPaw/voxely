import { describe, expect, it } from "vitest";
import type { SessionState } from "./api";
import { messagesFor } from "./i18n";
import {
  historyStatusLabel,
  overlayHudLabel,
  overlayHudVisible,
  overlayIsBusy,
  overlayLabel,
  overlayShouldRender,
  overlaySttAttempt,
  sessionStatusLabel,
} from "./session-copy";

const copy = messagesFor("en");
describe("session presentation lifecycle", () => {
  it.each<[SessionState, string, string, boolean, boolean, number | null]>([
    [{ kind: "idle" }, "", copy.overlayWaiting, false, false, null],
    [{ kind: "startingRecording" }, copy.overlayRecording, copy.overlayRecording, true, true, null],
    [{ kind: "recording" }, copy.overlayRecording, copy.overlayRecording, true, true, null],
    [{ kind: "stoppingRecording" }, copy.overlaySaving, copy.overlayProcessing, true, true, null],
    [{ kind: "saving" }, copy.overlaySaving, copy.overlayProcessing, true, true, null],
    [{ kind: "processingAudio" }, copy.overlayProcessing, copy.overlayProcessing, true, true, null],
    [
      { kind: "transcribing", attempt: 1 },
      copy.overlayTranscribing,
      copy.overlayTranscribing,
      true,
      true,
      1,
    ],
    [
      { kind: "transcribing", attempt: 2 },
      copy.overlayRetry.replace("{attempt}", "2"),
      copy.overlayRetry.replace("{attempt}", "2"),
      true,
      true,
      2,
    ],
    [
      { kind: "retryWaiting", attempt: 2, delayMs: 500 },
      copy.overlayRetry.replace("{attempt}", "3"),
      copy.overlayRetry.replace("{attempt}", "3"),
      true,
      true,
      3,
    ],
    [{ kind: "completed" }, "", copy.overlayReady, false, false, null],
    [
      { kind: "failed", code: "Interrupted", message: "failure" },
      copy.errInterrupted,
      copy.overlayError,
      true,
      false,
      null,
    ],
  ])("presents %j consistently", (state, overlay, status, visible, busy, attempt) => {
    expect(overlayLabel(state, copy)).toBe(overlay);
    expect(sessionStatusLabel(state, copy)).toBe(status);
    expect(overlayHudVisible(state)).toBe(visible);
    expect(overlayIsBusy(state)).toBe(busy);
    expect(overlaySttAttempt(state)).toBe(attempt);
    expect(overlayShouldRender(true, state)).toBe(visible);
    expect(overlayShouldRender(false, state)).toBe(false);
    expect(overlayHudLabel(false, state, copy, true)).toBe("");
    expect(overlayHudLabel(true, state, copy)).toBe(overlay);
    const limited = ["startingRecording", "recording", "stoppingRecording", "saving"].includes(
      state.kind,
    );
    expect(overlayHudLabel(true, state, copy, true)).toBe(
      limited ? copy.overlayRecordingLimit : overlay,
    );
  });
  it.each([
    ["processing", copy.overlayTranscribing],
    ["interrupted", copy.errInterrupted],
    ["completed", copy.overlayReady],
    ["failed", copy.overlayError],
    ["future-status", "future-status"],
  ])("preserves history status %s", (status, label) => {
    expect(historyStatusLabel(status, copy)).toBe(label);
  });
  it("uses Russian when a caller omits translation messages", () => {
    const state: SessionState = { kind: "recording" };
    expect(overlayLabel(state)).toBe(messagesFor("ru").overlayRecording);
    expect(sessionStatusLabel(state)).toBe(messagesFor("ru").overlayRecording);
    expect(overlayHudLabel(true, state)).toBe(messagesFor("ru").overlayRecording);
    expect(historyStatusLabel("completed")).toBe(messagesFor("ru").overlayReady);
  });
});
