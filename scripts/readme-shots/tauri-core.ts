import type {
  AppSettings,
  CompareState,
  DspPreset,
  MeterSample,
  Recording,
  SessionState,
  UsageStatistics,
} from "../../src/lib/api";

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

const optimizedPreset: DspPreset = {
  ...preset,
  id: "stt-optimized",
  name: "Quality (slower)",
  order: [
    { id: "highpass", kind: "highPass", enabled: true },
    { id: "rnnoise", kind: "rnnoise", enabled: true },
    { id: "compressor", kind: "compressor", enabled: true },
    { id: "expander", kind: "expander", enabled: false },
    { id: "gain", kind: "gain", enabled: true },
    { id: "gate", kind: "gate", enabled: false },
    { id: "limiter", kind: "limiter", enabled: true },
  ],
  highPass: { cutoffHz: 70, sampleRate: 48000 },
  compressor: {
    thresholdDb: -18,
    ratio: 1.8,
    attackMs: 10,
    releaseMs: 100,
    makeupDb: 1,
    sampleRate: 48000,
  },
  limiter: { thresholdDb: -0.8, releaseMs: 50, sampleRate: 48000 },
  rnnoiseMix: 0.6,
};

let settings: AppSettings = {
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
  storageLimit: "5gb",
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
  textReplacements: {
    enabled: true,
    rules: [
      { from: "voxily", to: "Voxely", caseSensitive: false },
      { from: "github", to: "GitHub", caseSensitive: false },
      { from: "open router", to: "OpenRouter", caseSensitive: false },
      { from: "web view", to: "WebView2", caseSensitive: false },
    ],
  },
  presets: [preset, optimizedPreset],
  firstRunComplete: true,
  uiLanguage: "en",
  compareModels: ["openai/gpt-transcribe", "openai/whisper-large-v3"],
};

const history: Recording[] = [
  {
    id: "rec-1",
    createdAt: new Date().toISOString(),
    durationMs: 40_000,
    rawAudioPath: "a.wav",
    processedAudioPath: "b.wav",
    transcript:
      "Quick notes from today's planning call. We agreed to finish onboarding before starting the new dashboard. The first screen should explain the recording shortcut without making people read a whole guide.\n\nI'll put together a prototype tomorrow morning. Please try it when you have a moment and note anything that feels confusing. Let's meet on Thursday to decide what actually needs to be in the release.",
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
    createdAt: new Date(Date.now() - 35 * 60_000).toISOString(),
    durationMs: 4000,
    rawAudioPath: "c.wav",
    processedAudioPath: "d.wav",
    transcript: "Sounds good, see you at eleven!",
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
  {
    id: "rec-3",
    createdAt: new Date(Date.now() - 2 * 60 * 60_000).toISOString(),
    durationMs: 68_000,
    rawAudioPath: "e.wav",
    processedAudioPath: "f.wav",
    transcript:
      "A few notes from today's planning call. We agreed to finish the onboarding changes before starting on the new dashboard. The first screen should explain what happens after you press the recording shortcut, without making people read a whole guide.\n\nFor the dashboard, I'd like to see the daily totals first, with a simple way to switch between the last week and the last month. We should also keep the original recordings available so it's easy to check a transcript later.\n\nI'll put together a small prototype tomorrow morning. Once everyone has had a chance to try it, let's meet on Thursday and decide what actually needs to be in the release.",
    status: "completed",
    provider: "openrouter",
    model: "openai/gpt-transcribe",
    attemptCount: 1,
    lastErrorCode: null,
    lastErrorMessage: null,
    cost: 0.0067,
    generationId: "gen-3",
    latencyMs: 1950,
  },
  {
    id: "rec-4",
    createdAt: new Date(Date.now() - 26 * 60 * 60_000).toISOString(),
    durationMs: 35_000,
    rawAudioPath: "g.wav",
    processedAudioPath: "h.wav",
    transcript:
      "Remember to pick up coffee and oat milk on the way home. There's still enough pasta for dinner, so we only need tomatoes and something for the salad. If the little bakery is still open, grab a loaf of bread as well. I'll be back around seven, but don't wait for me if you're hungry.",
    status: "completed",
    provider: "openrouter",
    model: "openai/whisper-large-v3",
    attemptCount: 1,
    lastErrorCode: null,
    lastErrorMessage: null,
    cost: 0.0048,
    generationId: "gen-4",
    latencyMs: 1700,
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

const handlers = new Map<string, Set<(event: { payload: unknown }) => void>>();
let compare: CompareState = {
  recording: false,
  running: false,
  nonce: 0,
  listenPath: null,
  sttPath: null,
  runId: null,
  slots: [],
};
export function fixtureListen(event: string, handler: (event: { payload: unknown }) => void) {
  const listeners = handlers.get(event) ?? new Set();
  listeners.add(handler);
  handlers.set(event, listeners);
  return () => {
    listeners.delete(handler);
  };
}
function fixtureEmit(event: string, payload: unknown) {
  for (const handler of handlers.get(event) ?? []) handler({ payload: structuredClone(payload) });
}

function fixtureAudioUrl(durationSeconds = 0.1) {
  const pcm = new Uint8Array(44 + Math.round(16_000 * durationSeconds) * 2);
  const view = new DataView(pcm.buffer);
  const text = (offset: number, value: string) =>
    [...value].forEach((char, index) => {
      pcm[offset + index] = char.charCodeAt(0);
    });
  text(0, "RIFF");
  view.setUint32(4, pcm.length - 8, true);
  text(8, "WAVEfmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, 16000, true);
  view.setUint32(28, 32000, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  text(36, "data");
  view.setUint32(40, pcm.length - 44, true);
  let binary = "";
  for (let offset = 0; offset < pcm.length; offset += 8192) {
    binary += String.fromCharCode(...pcm.subarray(offset, offset + 8192));
  }
  return `data:audio/wav;base64,${btoa(binary)}#`;
}

function fixtureUsageStatistics(start: string, end: string): UsageStatistics {
  const first = new Date(start);
  const endExclusive = new Date(end);
  const daily: UsageStatistics["daily"] = [];
  const dictationsByDay = [
    0, 1, 0, 2, 0, 1, 0, 1, 0, 3, 0, 1, 0, 2, 1, 0, 0, 2, 0, 1, 0, 0, 3, 0, 2, 1, 0, 0, 2, 1,
  ];
  const cursor = new Date(first.getFullYear(), first.getMonth(), first.getDate());

  for (let index = 0; cursor < endExclusive; index += 1) {
    const dictations = dictationsByDay[index % dictationsByDay.length] ?? 0;
    const apiRequests = dictations + (dictations > 0 && index % 4 === 1 ? 1 : 0);
    const reportedCostUsd = Number(
      (apiRequests * 0.008 + (dictations > 0 ? 0.0002 : 0)).toFixed(6),
    );
    daily.push({
      date: `${cursor.getFullYear()}-${String(cursor.getMonth() + 1).padStart(2, "0")}-${String(cursor.getDate()).padStart(2, "0")}`,
      dictations,
      apiRequests,
      reportedCostUsd,
    });
    cursor.setDate(cursor.getDate() + 1);
  }

  const totals = daily.reduce(
    (sum, item) => ({
      dictations: sum.dictations + item.dictations,
      apiRequests: sum.apiRequests + item.apiRequests,
      reportedCostUsd: Number((sum.reportedCostUsd + item.reportedCostUsd).toFixed(6)),
    }),
    { dictations: 0, apiRequests: 0, reportedCostUsd: 0 },
  );
  const primaryDictations = Math.round(totals.dictations * 0.7);
  const primaryRequests = Math.round(totals.apiRequests * 0.71);
  const primaryCost = Number((totals.reportedCostUsd * 0.69).toFixed(6));
  const unpricedAttempts = Math.min(3, totals.apiRequests);
  const primaryUnpriced = Math.min(2, unpricedAttempts);

  return {
    ...totals,
    completed: Math.max(0, totals.dictations - 2),
    failed: Math.min(2, totals.dictations),
    interrupted: 0,
    unpricedAttempts,
    audioDurationMs: totals.dictations * 118_000,
    daily,
    models: [
      {
        model: "openai/gpt-transcribe",
        dictations: primaryDictations,
        apiRequests: primaryRequests,
        reportedCostUsd: primaryCost,
        unpricedAttempts: primaryUnpriced,
      },
      {
        model: "openai/whisper-large-v3",
        dictations: totals.dictations - primaryDictations,
        apiRequests: totals.apiRequests - primaryRequests,
        reportedCostUsd: Number((totals.reportedCostUsd - primaryCost).toFixed(6)),
        unpricedAttempts: unpricedAttempts - primaryUnpriced,
      },
    ].filter((model) => model.dictations > 0 || model.apiRequests > 0),
  };
}

export async function invoke<T>(cmd: string, _args?: Record<string, unknown>): Promise<T> {
  switch (cmd) {
    case "get_settings":
      return structuredClone(settings) as T;
    case "get_overlay_snapshot":
      return {
        revision: 1,
        visible: window.location.search.includes("overlay"),
        state: sessionFromSearch(),
      } as T;
    case "get_runtime_info":
      return {
        localBuild: false,
        buildDate: new Date().toISOString().slice(0, 10),
        settingsRecovered: false,
      } as T;
    case "save_settings":
      settings = structuredClone({ ...settings, ...(_args?.settings as object | undefined) });
      fixtureEmit("settings://changed", settings);
      return structuredClone(settings) as T;
    case "acknowledge_first_run_disclosure":
      settings = { ...settings, firstRunComplete: true };
      fixtureEmit("settings://changed", settings);
      return structuredClone(settings) as T;
    case "recording_audio_url":
      return fixtureAudioUrl() as T;
    case "get_session_state":
      return sessionFromSearch() as T;
    case "list_history":
      return structuredClone(history) as T;
    case "get_usage_statistics":
      return fixtureUsageStatistics(String(_args?.start ?? ""), String(_args?.end ?? "")) as T;
    case "list_history_summaries":
    case "search_history":
      return {
        items: history,
        nextCursor: null,
        total: history.length,
        hasMore: false,
      } as T;
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
      return undefined as T;
    case "start_filter_sample":
      fixtureEmit("filter://sample", true);
      return undefined as T;
    case "get_model_compare":
      return structuredClone(compare) as T;
    case "start_model_compare":
      compare = {
        recording: true,
        running: false,
        nonce: compare.nonce + 1,
        listenPath: null,
        sttPath: null,
        runId: null,
        slots: [],
      };
      fixtureEmit("compare://state", compare);
      return undefined as T;
    case "stop_model_compare":
      compare = {
        ...compare,
        recording: false,
        listenPath: fixtureAudioUrl(22),
        sttPath: fixtureAudioUrl(22),
      };
      fixtureEmit("compare://state", compare);
      return structuredClone(compare) as T;
    case "run_model_compare":
      compare = {
        ...compare,
        runId: `fixture-run-${compare.nonce}`,
        slots: settings.compareModels!.map((model, index) => ({
          slotId: `fixture-${index}`,
          model,
          status: "done",
          text:
            index === 0
              ? "Let's move the design review to Thursday afternoon. That gives everyone a little more time to try the prototype and write down any questions. I'll send an updated invitation once we've agreed on a time."
              : "Let's move the design review to Thursday afternoon. That gives everyone a little more time to try the prototype and write down any questions. I'll send an updated invitation once we have agreed on a time.",
          error: null,
          attempt: 1,
          cost: null,
          latencyMs: 1000,
          clipNonce: compare.nonce,
          runId: `fixture-run-${compare.nonce}`,
        })),
      };
      fixtureEmit("compare://state", compare);
      return structuredClone(compare) as T;
    case "clear_model_compare":
      compare = {
        recording: false,
        running: false,
        nonce: 0,
        listenPath: null,
        sttPath: null,
        runId: null,
        slots: [],
      };
      fixtureEmit("compare://state", compare);
      return undefined as T;
    case "cancel_model_compare":
      compare = {
        ...compare,
        running: false,
        slots: compare.slots.map((slot) => ({
          ...slot,
          status: slot.status === "running" ? "cancelled" : slot.status,
        })),
      };
      fixtureEmit("compare://state", compare);
      return structuredClone(compare) as T;
    case "play_cue":
    case "preview_error_notification":
    case "open_audio_dir":
    case "open_github":
    case "open_logs":
      return undefined as T;
    case "preview_dsp":
    case "stop_filter_sample":
      if (cmd === "stop_filter_sample") fixtureEmit("filter://sample", false);
      return {
        originalPath: fixtureAudioUrl(),
        processedPath: fixtureAudioUrl(),
        originalDataUrl: "",
        processedDataUrl: "",
        peak: 0.42,
        rms: 0.11,
        clipCount: 0,
        nonce: 1,
      } as T;
    case "check_for_updates":
      return "none" as T;
    case "install_update":
      return "installed" as T;
    case "discover_models":
      return [{ id: "openai/gpt-transcribe", name: "GPT Transcribe" }] as T;
    default:
      return undefined as T;
  }
}

export function convertFileSrc(path: string): string {
  return path.startsWith("data:") ? path : fixtureAudioUrl();
}
