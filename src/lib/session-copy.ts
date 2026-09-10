import type { SessionState } from "./api";
import { localizedError, messagesFor, type Messages } from "./i18n";

export function overlayLabel(state: SessionState, copy: Messages = messagesFor("ru")): string {
  switch (state.kind) {
    case "idle":
      return copy.overlayReady;
    case "startingRecording":
    case "recording":
      return copy.overlayRecording;
    case "stoppingRecording":
    case "saving":
      return copy.overlaySaving;
    case "processingAudio":
      return copy.overlayProcessing;
    case "transcribing":
      return copy.overlayTranscribing;
    case "retryWaiting":
      return copy.overlayRetry.replace("{attempt}", String(state.attempt));
    case "completed":
      return copy.overlayReady;
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
      return copy.overlayTranscribing;
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
      return copy.overlayProcessing;
    case "completed":
      return copy.overlayReady;
    case "failed":
      return copy.overlayError;
    default:
      return status;
  }
}
