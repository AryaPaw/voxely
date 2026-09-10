import type { AppSettings, DspPreset, MeterSample, Recording, SessionState } from "../../src/lib/api";

const preset: DspPreset = {
  id: "stt-fast",
  name: "Быстрая диктовка",
  order: [
    { id: "highpass", kind: "highPass", enabled: true },
    { id: "gain", kind: "gain", enabled: true },
    { id: "limiter", kind: "limiter", enabled: true },
  ],
  highPass: { cutoffHz: 80, sampleRate: 48000 },
  gain: { db: 1.5 },
  compressor: {
    thresholdDb: -18,
    ratio: 3,
    attackMs: 6,
    releaseMs: 120,
    makeupDb: 3,
    sampleRate: 48000,
  },
  expander: {
    thresholdDb: -45,
    ratio: 2,
    attackMs: 8,
    releaseMs: 80,
    makeupDb: 0,
    sampleRate: 48000,
  },
  gate: {
    openThresholdDb: -48,
    closeThresholdDb: -52,
    holdMs: 250,
    releaseMs: 200,
    sampleRate: 48000,
  },
  limiter: { thresholdDb: -1, releaseMs: 40, sampleRate: 48000 },
};

const settings: AppSettings = {
  hotkey: "Ctrl+Shift+Space",
  startWithWindows: false,
  closeToTray: true,
  notifications: true,
  inputDevice: "default",
  keepOriginalRecordings: true,
  theme: "dark",
  language: "ru",
  model: "openai/gpt-transcribe",
  customModel: null,
  insertionMode: "unicode",
  retention: "3d",
  storageLimit: "1gb",
  debugLogging: false,
  retry: {
    automaticRetries: true,
    additionalRetries: 2,
    connectTimeoutMs: 8000,
    requestTimeoutMs: 20000,
    initialRetryDelayMs: 500,
    maxRetryDelayMs: 8000,
    totalOperationTimeoutMs: 720000,
  },
  activePresetId: "stt-fast",
  presets: [preset],
  firstRunComplete: true,
  uiLanguage: "ru",
  autoUpdateEnabled: true,
};

const history: Recording[] = [
  {
    id: "rec-1",
    createdAt: new Date("2026-09-10T18:12:00Z").toISOString(),
    durationMs: 8200,
    rawAudioPath: "a.wav",
    processedAudioPath: "b.wav",
    transcript: "Открой вчерашний отчёт и пришли ссылку в чат.",
    status: "completed",
    provider: "openrouter",
    model: "openai/gpt-transcribe",
    attemptCount: 1,
    lastErrorCode: null,
    lastErrorMessage: null,
    cost: 0.0042,
    generationId: "gen-1",
    latencyMs: 1840,
  },
  {
    id: "rec-2",
    createdAt: new Date("2026-09-10T17:41:00Z").toISOString(),
    durationMs: 5400,
    rawAudioPath: "c.wav",
    processedAudioPath: "d.wav",
    transcript: "Напомни про созвон в пятницу в 11:00.",
    status: "completed",
    provider: "openrouter",
    model: "openai/gpt-transcribe",
    attemptCount: 1,
    lastErrorCode: null,
    lastErrorMessage: null,
    cost: 0.0031,
    generationId: "gen-2",
    latencyMs: 1210,
  },
];

function sessionFromSearch(): SessionState {
  const search = window.location.search;
  if (search.includes("overlay")) {
    if (search.includes("transcribing")) {
      return { kind: "transcribing", attempt: 1 };
    }
    return { kind: "recording" };
  }
  return { kind: "idle" };
}

const meter: MeterSample = {
  rms: 0.14,
  peak: 0.38,
  levels: Array.from({ length: 28 }, (_, index) => 0.18 + ((index % 7) / 18) * 0.45),
};

export async function invoke<T>(cmd: string): Promise<T> {
  switch (cmd) {
    case "get_settings":
      return settings as T;
    case "save_settings":
      return settings as T;
    case "get_session_state":
      return sessionFromSearch() as T;
    case "list_history":
      return history as T;
    case "api_key_configured":
      return true as T;
    case "list_microphones":
      return [
        { id: "mic-internal", name: "Микрофон ноутбука", isDefault: true },
        { id: "mic-1", name: "Studio Mic", isDefault: false },
      ] as T;
    case "get_meter":
      return meter as T;
    case "start_input_meter":
    case "stop_input_meter":
    case "start_filter_sample":
    case "cancel_dictation":
    case "open_audio_dir":
    case "open_github":
    case "open_logs":
      return undefined as T;
    case "preview_dsp":
    case "stop_filter_sample":
      return {
        originalPath: "a.wav",
        processedPath: "b.wav",
        originalDataUrl: "",
        processedDataUrl: "",
        peak: 0.42,
        rms: 0.11,
        clipCount: 0,
        nonce: 1,
      } as T;
    case "check_for_updates":
      return "upToDate" as T;
    case "discover_models":
      return [{ id: "openai/gpt-transcribe", name: "GPT Transcribe" }] as T;
    default:
      return undefined as T;
  }
}

export function convertFileSrc(path: string): string {
  return path;
}
