import { invoke } from "@tauri-apps/api/core";

export type SessionState =
  | { kind: "idle" }
  | { kind: "startingRecording" }
  | { kind: "recording" }
  | { kind: "stoppingRecording" }
  | { kind: "saving" }
  | { kind: "processingAudio" }
  | { kind: "transcribing"; attempt: number }
  | { kind: "retryWaiting"; attempt: number; delayMs: number }
  | { kind: "completed" }
  | { kind: "failed"; message: string; code: string };

export interface Recording {
  id: string;
  createdAt: string;
  durationMs: number;
  rawAudioPath: string | null;
  processedAudioPath: string | null;
  transcript: string | null;
  status: "processing" | "completed" | "failed" | "interrupted";
  provider: string;
  model: string;
  attemptCount: number;
  lastErrorCode: string | null;
  lastErrorMessage: string | null;
  cost: number | null;
  generationId: string | null;
  latencyMs: number | null;
}

export interface RetrySettings {
  automaticRetries: boolean;
  additionalRetries: number;
  connectTimeoutMs: number;
  requestTimeoutMs: number;
  initialRetryDelayMs: number;
  maxRetryDelayMs: number;
  totalOperationTimeoutMs: number;
}

export type FilterKind =
  "highPass" | "rnnoise" | "gain" | "compressor" | "expander" | "gate" | "limiter";

export interface FilterSlot {
  id: string;
  kind: FilterKind;
  enabled: boolean;
}

export interface DspPreset {
  id: string;
  name: string;
  order: FilterSlot[];
  highPass: { cutoffHz: number; sampleRate: number };
  gain: { db: number };
  compressor: {
    thresholdDb: number;
    ratio: number;
    attackMs: number;
    releaseMs: number;
    makeupDb: number;
    sampleRate: number;
  };
  expander: {
    thresholdDb: number;
    ratio: number;
    attackMs: number;
    releaseMs: number;
    makeupDb: number;
    sampleRate: number;
  };
  gate: {
    openThresholdDb: number;
    closeThresholdDb: number;
    holdMs: number;
    releaseMs: number;
    sampleRate: number;
  };
  limiter: { thresholdDb: number; releaseMs: number; sampleRate: number };
  rnnoiseMix: number;
}

export interface MicTune {
  gainDb: number;
  highpassHz: number;
  denoise: number;
  punch: number;
}

export const defaultMicTune: MicTune = {
  gainDb: 1.5,
  highpassHz: 80,
  denoise: 0,
  punch: 18,
};

export interface DspPreview {
  originalPath: string;
  processedPath: string;
  originalDataUrl?: string;
  processedDataUrl?: string;
  peak: number;
  rms: number;
  clipCount: number;
  nonce: number;
}

export interface AppSettings {
  hotkey: string;
  startWithWindows: boolean;
  closeToTray: boolean;
  notifications: boolean;
  inputDevice: string;
  keepOriginalRecordings: boolean;
  theme: string;
  language: string;
  model: string;
  customModel: string | null;
  insertionMode: string;
  retention: string;
  storageLimit: string;
  debugLogging: boolean;
  retry: RetrySettings;
  activePresetId: string;
  presets: DspPreset[];
  firstRunComplete: boolean;
  configRevision?: number;
  micTune?: MicTune;
  uiLanguage?: string;
  autoUpdateEnabled?: boolean;
  compareModels?: string[];
}

export interface MeterSample {
  rms: number;
  peak: number;
  levels?: number[];
}

export interface CompareSlot {
  model: string;
  status: string;
  text: string | null;
  error: string | null;
  attempt: number;
  cost: number | null;
  latencyMs: number | null;
}

export interface CompareState {
  recording: boolean;
  running: boolean;
  nonce: number;
  listenPath: string | null;
  sttPath: string | null;
  runId: string | null;
  slots: CompareSlot[];
}

export type CueKind = "start" | "stop" | "cancel";

export interface RuntimeInfo {
  localBuild: boolean;
}

export const api = {
  session: () => invoke<SessionState>("get_session_state"),
  settings: () => invoke<AppSettings>("get_settings"),
  runtimeInfo: () => invoke<RuntimeInfo>("get_runtime_info"),
  saveSettings: (settings: AppSettings) => invoke<AppSettings>("save_settings", { settings }),
  history: () => invoke<Recording[]>("list_history"),
  recording: (id: string) => invoke<Recording | null>("get_recording", { id }),
  deleteItem: (id: string) => invoke<void>("delete_history_item", { id }),
  deleteAll: () => invoke<void>("delete_all_history"),
  mics: () => invoke<Array<{ id: string; name: string; isDefault: boolean }>>("list_microphones"),
  meter: () => invoke<MeterSample>("get_meter"),
  startInputMeter: () => invoke<void>("start_input_meter"),
  stopInputMeter: () => invoke<void>("stop_input_meter"),
  previewDsp: () => invoke<DspPreview>("preview_dsp"),
  startFilterSample: () => invoke<void>("start_filter_sample"),
  stopFilterSample: () => invoke<DspPreview>("stop_filter_sample"),
  checkForUpdates: () => invoke<string>("check_for_updates"),
  keyConfigured: () => invoke<boolean>("api_key_configured"),
  storeKey: (key: string) => invoke<boolean>("store_api_key", { key }),
  testConnection: () => invoke<number>("test_openrouter"),
  models: () => invoke<Array<{ id: string; name: string }>>("discover_models"),
  toggle: () => invoke<void>("toggle_dictation"),
  cancel: () => invoke<void>("cancel_dictation"),
  setHotkeyCapture: (capturing: boolean) => invoke<void>("set_hotkey_capture", { capturing }),
  retry: (id: string) => invoke<Recording>("retry_recording", { id }),
  openLogs: () => invoke<void>("open_logs"),
  openSettingsDir: () => invoke<void>("open_settings_dir"),
  resetSettings: (wipeApiKey: boolean) => invoke<AppSettings>("reset_settings", { wipeApiKey }),
  obsPreview: () =>
    invoke<Array<{ sourceName: string; unsupported: string[] }>>("preview_obs_import"),
  importObs: (sourceName: string, presetName: string) =>
    invoke("import_obs_preset", { sourceName, presetName }),
  copy: (text: string) => invoke<void>("copy_transcript", { text }),
  insert: (text: string) => invoke<void>("insert_transcript", { text }),
  audioPath: (id: string) => invoke<string | null>("recording_audio_url", { id }),
  openAudioDir: () => invoke<void>("open_audio_dir"),
  openGithub: (page?: string) => invoke<void>("open_github", { page }),
  startModelCompare: () => invoke<void>("start_model_compare"),
  stopModelCompare: () => invoke<CompareState>("stop_model_compare"),
  runModelCompare: () => invoke<CompareState>("run_model_compare"),
  getModelCompare: () => invoke<CompareState>("get_model_compare"),
  clearModelCompare: () => invoke<void>("clear_model_compare"),
  playCue: (kind: CueKind) => invoke<void>("play_cue", { kind }),
};
