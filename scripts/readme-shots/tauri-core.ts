import type { AppSettings, DspPreset, MeterSample, Recording, SessionState } from "../../src/lib/api";

const preset: DspPreset = {
  id: "stt-fast",
  name: "Fast dictation",
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
  rnnoiseMix: 1,
};

const settings: AppSettings = {
  hotkey: "Ctrl+Shift+Space",
  startWithWindows: false,
  closeToTray: true,
  notifications: true,
  inputDevice: "default",
  keepOriginalRecordings: true,
  theme: "dark",
  language: "en",
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
  activePresetId: "stt-optimized",
  presets: [preset],
  firstRunComplete: true,
  uiLanguage: "en",
  compareModels: ["openai/gpt-transcribe", "openai/whisper-large-v3"],
};

const history: Recording[] = [
  {
    id: "rec-1",
    createdAt: new Date("2026-09-10T18:12:00Z").toISOString(),
    durationMs: 8200,
    rawAudioPath: "a.wav",
    processedAudioPath: "b.wav",
    transcript: "Open yesterday's report and send the link in chat.",
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
    transcript: "Remind me about the Friday call at 11:00.",
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
  levels: [
    0.05, 0.07, 0.1, 0.16, 0.28, 0.44, 0.36, 0.22, 0.3, 0.52, 0.4, 0.24, 0.14, 0.2, 0.34, 0.48,
    0.38, 0.22, 0.12, 0.18, 0.26, 0.2, 0.12, 0.08, 0.06, 0.05, 0.04, 0.04,
  ],
};

export async function invoke<T>(cmd: string): Promise<T> {
  switch (cmd) {
    case "get_settings":
      return settings as T;
    case "get_runtime_info":
      return { localBuild: false } as T;
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
        { id: "mic-internal", name: "Laptop microphone", isDefault: true },
        { id: "mic-1", name: "Studio Mic", isDefault: false },
      ] as T;
    case "get_meter":
      return meter as T;
    case "start_input_meter":
    case "stop_input_meter":
    case "start_filter_sample":
    case "get_model_compare":
      return {
        recording: false,
        running: false,
        nonce: 0,
        listenPath: null,
        sttPath: null,
        runId: null,
        slots: [],
      } as T;
    case "start_model_compare":
    case "clear_model_compare":
    case "play_cue":
    case "preview_error_notification":
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
