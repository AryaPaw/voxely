import type { SessionState } from "./api";
import { localizedError, messagesFor, type Messages } from "./i18n";

export function overlayRetryDisplayAttempt(failedAttempt: number): number {
  return failedAttempt + 1;
}

export function overlaySttAttempt(state: SessionState): number | null {
  switch (state.kind) {
    case "transcribing":
      return state.attempt;
    case "retryWaiting":
      return overlayRetryDisplayAttempt(state.attempt);
    case "idle":
    case "startingRecording":
    case "recording":
    case "stoppingRecording":
    case "saving":
    case "processingAudio":
    case "completed":
    case "failed":
      return null;
    default: {
      const _never: never = state;
      return _never;
    }
  }
}

export function overlayLabel(state: SessionState, copy: Messages = messagesFor("ru")): string {
  switch (state.kind) {
    case "idle":
      return "";
    case "startingRecording":
    case "recording":
      return copy.overlayRecording;
    case "stoppingRecording":
    case "saving":
      return copy.overlaySaving;
    case "processingAudio":
      return copy.overlayProcessing;
    case "transcribing":
    case "retryWaiting": {
      const attempt = overlaySttAttempt(state);
      if (attempt == null || attempt <= 1) {
        return copy.overlayTranscribing;
      }
      return copy.overlayRetry.replace("{attempt}", String(attempt));
    }
    case "completed":
      return "";
    case "failed":
      return localizedError(state.code, copy, state.message);
    default: {
      const _never: never = state;
      return _never;
    }
  }
}

export function sessionStatusLabel(
  state: SessionState,
  copy: Messages = messagesFor("ru"),
): string {
  switch (state.kind) {
    case "idle":
      return copy.overlayWaiting;
    case "startingRecording":
    case "recording":
      return copy.overlayRecording;
    case "stoppingRecording":
    case "saving":
    case "processingAudio":
      return copy.overlayProcessing;
    case "transcribing":
    case "retryWaiting":
      return overlayLabel(state, copy);
    case "completed":
      return copy.overlayReady;
    case "failed":
      return copy.overlayError;
    default: {
      const _never: never = state;
      return _never;
    }
  }
}

export function overlayHudLabel(
  visible: boolean,
  state: SessionState,
  copy: Messages = messagesFor("ru"),
  limitReached = false,
): string {
  if (!visible) {
    return "";
  }
  if (
    limitReached &&
    (state.kind === "recording" ||
      state.kind === "startingRecording" ||
      state.kind === "stoppingRecording" ||
      state.kind === "saving")
  ) {
    return copy.overlayRecordingLimit;
  }
  return overlayLabel(state, copy);
}

export function overlayShouldRender(visible: boolean, state: SessionState): boolean {
  return visible && overlayHudVisible(state);
}

export function overlayHudVisible(state: SessionState): boolean {
  switch (state.kind) {
    case "idle":
    case "completed":
      return false;
    case "startingRecording":
    case "recording":
    case "stoppingRecording":
    case "saving":
    case "processingAudio":
    case "transcribing":
    case "retryWaiting":
    case "failed":
      return true;
    default: {
      const _never: never = state;
      return _never;
    }
  }
}

export function overlayIsBusy(state: SessionState): boolean {
  switch (state.kind) {
    case "startingRecording":
    case "recording":
    case "stoppingRecording":
    case "saving":
    case "processingAudio":
    case "transcribing":
    case "retryWaiting":
      return true;
    case "idle":
    case "completed":
    case "failed":
      return false;
    default: {
      const _never: never = state;
      return _never;
    }
  }
}

export function historyStatusLabel(status: string, copy: Messages = messagesFor("ru")): string {
  switch (status) {
    case "processing":
      return copy.overlayTranscribing;
    case "interrupted":
      return copy.errInterrupted;
    case "completed":
      return copy.overlayReady;
    case "failed":
      return copy.overlayError;
    default:
      return status;
  }
}
