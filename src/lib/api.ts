import { invoke } from "@tauri-apps/api/core";
import { invokeWhenManaged } from "./invoke-ready";

function ipc<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return invokeWhenManaged(
    (name, payload) => {
      if (payload === undefined) {
        return invoke<T>(name);
      }
      return invoke<T>(name, payload);
    },
    cmd,
    args,
  );
}

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
  buildDate: string;
}

export const api = {
  session: () => ipc<SessionState>("get_session_state"),
  settings: () => ipc<AppSettings>("get_settings"),
  runtimeInfo: () => ipc<RuntimeInfo>("get_runtime_info"),
  saveSettings: (settings: AppSettings) => ipc<AppSettings>("save_settings", { settings }),
  history: () => ipc<Recording[]>("list_history_summaries"),
  recording: (id: string) => ipc<Recording | null>("get_recording", { id }),
  deleteItem: (id: string) => ipc<void>("delete_history_item", { id }),
  deleteAll: () => ipc<void>("delete_all_history"),
  mics: () => ipc<Array<{ id: string; name: string; isDefault: boolean }>>("list_microphones"),
  meter: () => ipc<MeterSample>("get_meter"),
  startInputMeter: () => ipc<void>("start_input_meter"),
  stopInputMeter: () => ipc<void>("stop_input_meter"),
  previewDsp: () => ipc<DspPreview>("preview_dsp"),
  startFilterSample: () => ipc<void>("start_filter_sample"),
  stopFilterSample: () => ipc<DspPreview>("stop_filter_sample"),
  checkForUpdates: () => ipc<string>("check_for_updates"),
  installUpdate: () => ipc<string>("install_update"),
  keyConfigured: () => ipc<boolean>("api_key_configured"),
  storeKey: (key: string) => ipc<boolean>("store_api_key", { key }),
  testConnection: () => ipc<number>("test_openrouter"),
  models: () => ipc<Array<{ id: string; name: string }>>("discover_models"),
  toggle: () => ipc<void>("toggle_dictation"),
  cancel: () => ipc<void>("cancel_dictation"),
  setHotkeyCapture: (capturing: boolean) => ipc<void>("set_hotkey_capture", { capturing }),
  retry: (id: string) => ipc<Recording>("retry_recording", { id }),
  openLogs: () => ipc<void>("open_logs"),
  openSettingsDir: () => ipc<void>("open_settings_dir"),
  resetSettings: (wipeApiKey: boolean) => ipc<AppSettings>("reset_settings", { wipeApiKey }),
  obsPreview: () => ipc<Array<{ sourceName: string; unsupported: string[] }>>("preview_obs_import"),
  importObs: (sourceName: string, presetName: string) =>
    ipc("import_obs_preset", { sourceName, presetName }),
  copy: (text: string) => ipc<void>("copy_transcript", { text }),
  insert: (text: string) => ipc<void>("insert_transcript", { text }),
  audioPath: (id: string) => ipc<string | null>("recording_audio_url", { id }),
  openAudioDir: () => ipc<void>("open_audio_dir"),
  openGithub: (page?: string) => ipc<void>("open_github", { page }),
  startModelCompare: () => ipc<void>("start_model_compare"),
  stopModelCompare: () => ipc<CompareState>("stop_model_compare"),
  runModelCompare: () => ipc<CompareState>("run_model_compare"),
  getModelCompare: () => ipc<CompareState>("get_model_compare"),
  clearModelCompare: () => ipc<void>("clear_model_compare"),
  playCue: (kind: CueKind) => ipc<void>("play_cue", { kind }),
  previewErrorNotification: () => ipc<void>("preview_error_notification"),
};
