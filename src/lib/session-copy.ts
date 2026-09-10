import type { SessionState } from "./api";

export function overlayLabel(state: SessionState): string {
  switch (state.kind) {
    case "idle":
      return "Готово";
    case "startingRecording":
    case "recording":
      return "Запись";
    case "stoppingRecording":
    case "saving":
      return "Сохранение";
    case "processingAudio":
      return "Обработка";
    case "transcribing":
      return "Расшифровка";
    case "retryWaiting":
      return `Повтор ${state.attempt}`;
    case "completed":
      return "Готово";
    case "failed":
      return state.message;
    default: {
      const _never: never = state;
      return _never;
    }
  }
}

export function sessionStatusLabel(state: SessionState): string {
  switch (state.kind) {
    case "idle":
      return "Ожидание";
    case "startingRecording":
    case "recording":
      return "Запись";
    case "stoppingRecording":
    case "saving":
    case "processingAudio":
      return "Обработка";
    case "transcribing":
    case "retryWaiting":
      return "Расшифровка";
    case "completed":
      return "Готово";
    case "failed":
      return "Ошибка";
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

export function historyStatusLabel(status: string): string {
  switch (status) {
    case "processing":
      return "Обработка";
    case "completed":
      return "Готово";
    case "failed":
      return "Ошибка";
    default:
      return status;
  }
}
