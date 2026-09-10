export type UiLocale = "ru" | "en";

export function resolveUiLocale(uiLanguage: string, navigatorLanguage = "en"): UiLocale {
  if (uiLanguage === "ru" || uiLanguage === "en") {
    return uiLanguage;
  }
  return navigatorLanguage.toLowerCase().startsWith("ru") ? "ru" : "en";
}

const ru = {
  navHistory: "Записи",
  navGeneral: "Общие",
  navAudio: "Микрофон",
  navFilters: "Фильтры",
  navTranscription: "Расшифровка",
  navStorage: "Хранение",
  navAppearance: "Внешний вид",
  navAdvanced: "Дополнительно",
  historyTitle: "История",
  historyHint: "Диктовки хранятся локально на этом компьютере.",
  search: "Поиск по расшифровкам",
  folder: "Папка",
  settings: "Настройки",
  emptyHistory: "Записей пока нет. Нажмите {hotkey} в текстовом поле.",
  processing: "Расшифровка",
  copy: "Копировать",
  retry: "Повторить расшифровку",
  copied: "Скопировано",
  filtersTitle: "Фильтры",
  filtersIntro:
    "Сначала запишите короткую фразу на этой странице. Потом сравните оригинал и звук после фильтров.",
  recordSample: "Записать образец",
  stopSample: "Стоп",
  recordingSample: "Говорите… затем нажмите Стоп",
  playOriginal: "Оригинал",
  playProcessed: "После фильтров",
  uiLanguage: "Язык интерфейса",
  uiAuto: "Авто (система)",
  uiRu: "Русский",
  uiEn: "English",
  autoUpdate: "Автообновления",
  checkUpdates: "Проверить обновления",
  checking: "Проверка…",
  startWithWindows: "Запускать вместе с Windows",
  closeToTray: "Сворачивать в трей",
  notifications: "Уведомления",
  overlayReady: "Готово",
  overlayRecording: "Запись",
  overlaySaving: "Сохранение",
  overlayProcessing: "Обработка",
  overlayTranscribing: "Расшифровка",
  overlayRetry: "Повтор {attempt}",
  overlayWaiting: "Ожидание",
  overlayError: "Ошибка",
  overlayCancel: "Отменить запись",
  overlayCancelAria: "Наведите, чтобы отменить запись",
  loading: "Загрузка…",
  retryAction: "Повторить",
  defaultMic: "Системное по умолчанию",
  trayOpen: "Открыть",
  trayQuit: "Выход",
  errMicrophone: "Микрофон недоступен",
  errInvalidApiKey: "Нет API-ключа OpenRouter",
  errCancelled: "Отменено",
  errAudioCapture: "Не удалось записать звук",
  errAudioProcessing: "Не удалось обработать запись",
  errStorage: "Не удалось сохранить данные",
  errInvalidModel: "Неверная модель",
  errRequestValidation: "Неверный запрос",
  errNetwork: "Нет сети",
  errConnection: "Нет соединения с OpenRouter",
  errTimeout: "Превышено время ожидания",
  errRateLimited: "Слишком много запросов",
  errProvider: "Провайдер недоступен",
  errServer: "Ошибка сервера OpenRouter",
  errMalformed: "Некорректный ответ",
  errTooLarge: "Запись слишком большая",
  errRetryDeadline: "Истекло время повторов",
  errInsert: "Не удалось вставить текст",
  errHotkey: "Не удалось зарегистрировать хоткей",
  errIllegalTransition: "Недопустимое состояние сессии",
  errTranscriptionInProgress: "Расшифровка уже идёт",
  errInterrupted: "Запись прервана. Можно повторить расшифровку",
  errUnknown: "Неизвестная ошибка",
  dateLocale: "ru-RU",
  secondsAbbrev: "с",
  addApiKey: "Добавить ключ",
  missingApiKeyBanner: "Нет API-ключа OpenRouter. Запись сохранится, расшифровка не отправится.",
  details: "Сведения",
  delete: "Удалить",
  transcriptUnavailable: "Расшифровка недоступна.",
  pause: "Пауза",
  listen: "Слушать",
  modelLabel: "Модель",
  latencyLabel: "Задержка",
  costLabel: "Стоимость",
  errorLabel: "Ошибка",
  globalHotkey: "Глобальный хоткей",
  hotkeyPrompt: "Нажмите сочетание… Esc отмена",
  inputDevice: "Устройство ввода",
  theme: "Тема",
  themeSystem: "Системная",
  themeLight: "Светлая",
  themeDark: "Тёмная",
  insertMode: "Вставка текста",
  insertAuto: "Авто",
  insertSendInput: "SendInput",
  insertClipboard: "Буфер обмена",
  insertHint:
    "Авто — рекомендуемый режим: вставка в то окно, где вы говорили. Буфер обмена надёжнее в части приложений, но затирает то, что уже скопировано.",
  debugLogs: "Отладочные логи",
  openLogs: "Открыть логи",
  keepRecordings: "Хранить записи",
  retain1d: "1 день",
  retain3d: "3 дня",
  retain7d: "7 дней",
  retain30d: "30 дней",
  retain90d: "90 дней",
  retainForever: "Всегда",
  storageLimit: "Лимит места",
  limit500mb: "500 МБ",
  limit1gb: "1 ГБ",
  limit5gb: "5 ГБ",
  limitUnlimited: "Без лимита",
  keepOriginals: "Хранить исходные записи (лучше для прослушивания)",
  deleteAllHistory: "Удалить всю историю",
  deleteAllConfirm: "Удалить всю историю?",
  deleteAllCannotUndo: "Это нельзя отменить.",
  cancel: "Отмена",
  apiKeyHint: "Ключ хранится в Windows Credential Manager. В интерфейсе видно только:",
  keySaved: "ключ сохранён",
  keyMissing: "ключа нет",
  apiKey: "API-ключ",
  replaceKey: "Заменить ключ",
  saveKey: "Сохранить ключ",
  keySavedToast: "Ключ сохранён",
  keyNotSaved: "Ключ не сохранён",
  testConnection: "Проверить соединение",
  noConnection: "Нет соединения",
  timeoutHint:
    "Таймаут одного запроса растёт вместе с длительностью записи, до 15 минут. Общий лимит покрывает длинные диктовки и повторы.",
  language: "Язык",
  autoRetries: "Автоматические повторы",
  extraAttempts: "Дополнительные попытки (1 запрос + столько повторов)",
  connectTimeout: "Таймаут соединения (мс)",
  requestTimeout: "Таймаут запроса (мс)",
  initialDelay: "Начальная пауза (мс)",
  maxDelay: "Максимальная пауза (мс)",
  totalLimit: "Общий лимит операции (мс)",
  activePreset: "Активный пресет",
  gainDb: "Громкость {value} дБ",
  highpass: "Срез низов {value} Гц",
  denoise: "Шумодав {value}%",
  punch: "Компрессия {value}%",
  importObs: "Импорт из OBS",
  obsMicMissing: "Источник микрофона OBS не найден",
  unsupported: "Не поддерживается: {item}",
  sampleFailed: "Не удалось записать образец",
  micLevel: "Уровень микрофона",
  meterLive: "Сейчас слышно этот микрофон.",
  meterIdle: "Полоска оживёт, когда начнёте запись образца.",
  peakRms: "Пик {peak}%, RMS {rms}%",
  clipping: ", клиппинг {count}",
  loadFailed: "Не удалось загрузить настройки",
  historyFailed: "Не удалось загрузить историю",
  settingsFailed: "Не удалось сохранить настройки",
  updateAvailable: "Доступно обновление",
  updateNone: "Обновлений нет",
  updateBusy: "Обновление уже выполняется",
  updateDeferred: "Обновление отложено до конца диктовки",
  updateFailed: "Не удалось проверить обновления",
  updateInstalled: "Обновление установлено. Перезапуск…",
};

const en = {
  navHistory: "Recordings",
  navGeneral: "General",
  navAudio: "Microphone",
  navFilters: "Filters",
  navTranscription: "Transcription",
  navStorage: "Storage",
  navAppearance: "Appearance",
  navAdvanced: "Advanced",
  historyTitle: "History",
  historyHint: "Dictations stay on this computer.",
  search: "Search transcripts",
  folder: "Folder",
  settings: "Settings",
  emptyHistory: "No recordings yet. Press {hotkey} in a text field.",
  processing: "Transcribing",
  copy: "Copy",
  retry: "Retry transcription",
  copied: "Copied",
  filtersTitle: "Filters",
  filtersIntro:
    "Record a short phrase on this page, then compare the original with the filtered sound.",
  recordSample: "Record sample",
  stopSample: "Stop",
  recordingSample: "Speak… then press Stop",
  playOriginal: "Original",
  playProcessed: "After filters",
  uiLanguage: "Interface language",
  uiAuto: "Auto (system)",
  uiRu: "Русский",
  uiEn: "English",
  autoUpdate: "Automatic updates",
  checkUpdates: "Check for updates",
  checking: "Checking…",
  startWithWindows: "Start with Windows",
  closeToTray: "Minimize to tray",
  notifications: "Notifications",
  overlayReady: "Ready",
  overlayRecording: "Recording",
  overlaySaving: "Saving",
  overlayProcessing: "Processing",
  overlayTranscribing: "Transcribing",
  overlayRetry: "Retry {attempt}",
  overlayWaiting: "Waiting",
  overlayError: "Error",
  overlayCancel: "Cancel recording",
  overlayCancelAria: "Hover to cancel recording",
  loading: "Loading…",
  retryAction: "Retry",
  defaultMic: "System default",
  trayOpen: "Open",
  trayQuit: "Quit",
  errMicrophone: "Microphone unavailable",
  errInvalidApiKey: "OpenRouter API key is missing",
  errCancelled: "Cancelled",
  errAudioCapture: "Could not record audio",
  errAudioProcessing: "Could not process the recording",
  errStorage: "Could not save data",
  errInvalidModel: "Invalid model",
  errRequestValidation: "Invalid request",
  errNetwork: "Network unavailable",
  errConnection: "Could not reach OpenRouter",
  errTimeout: "Request timed out",
  errRateLimited: "Too many requests",
  errProvider: "Provider unavailable",
  errServer: "OpenRouter server error",
  errMalformed: "Malformed response",
  errTooLarge: "Recording is too large to send",
  errRetryDeadline: "Retry deadline exceeded",
  errInsert: "Could not insert text",
  errHotkey: "Could not register the hotkey",
  errIllegalTransition: "Illegal session state",
  errTranscriptionInProgress: "Transcription is already running",
  errInterrupted: "Recording was interrupted. You can retry transcription",
  errUnknown: "Unknown error",
  dateLocale: "en-US",
  secondsAbbrev: "s",
  addApiKey: "Add key",
  missingApiKeyBanner:
    "OpenRouter API key is missing. The recording is saved, transcription is skipped.",
  details: "Details",
  delete: "Delete",
  transcriptUnavailable: "Transcription is unavailable.",
  pause: "Pause",
  listen: "Listen",
  modelLabel: "Model",
  latencyLabel: "Latency",
  costLabel: "Cost",
  errorLabel: "Error",
  globalHotkey: "Global hotkey",
  hotkeyPrompt: "Press a shortcut… Esc cancels",
  inputDevice: "Input device",
  theme: "Theme",
  themeSystem: "System",
  themeLight: "Light",
  themeDark: "Dark",
  insertMode: "Text insertion",
  insertAuto: "Auto",
  insertSendInput: "SendInput",
  insertClipboard: "Clipboard",
  insertHint:
    "Auto is the recommended mode: insert into the window where you spoke. Clipboard is more reliable in some apps, but overwrites whatever was already copied.",
  debugLogs: "Debug logs",
  openLogs: "Open logs",
  keepRecordings: "Keep recordings",
  retain1d: "1 day",
  retain3d: "3 days",
  retain7d: "7 days",
  retain30d: "30 days",
  retain90d: "90 days",
  retainForever: "Forever",
  storageLimit: "Storage limit",
  limit500mb: "500 MB",
  limit1gb: "1 GB",
  limit5gb: "5 GB",
  limitUnlimited: "Unlimited",
  keepOriginals: "Keep original recordings (better for playback)",
  deleteAllHistory: "Delete all history",
  deleteAllConfirm: "Delete all history?",
  deleteAllCannotUndo: "This cannot be undone.",
  cancel: "Cancel",
  apiKeyHint: "The key is stored in Windows Credential Manager. The interface only shows:",
  keySaved: "key saved",
  keyMissing: "no key",
  apiKey: "API key",
  replaceKey: "Replace key",
  saveKey: "Save key",
  keySavedToast: "Key saved",
  keyNotSaved: "Key was not saved",
  testConnection: "Test connection",
  noConnection: "No connection",
  timeoutHint:
    "A single request timeout grows with recording duration, up to 15 minutes. The overall limit covers long dictations and retries.",
  language: "Language",
  autoRetries: "Automatic retries",
  extraAttempts: "Extra attempts (1 request plus this many retries)",
  connectTimeout: "Connect timeout (ms)",
  requestTimeout: "Request timeout (ms)",
  initialDelay: "Initial delay (ms)",
  maxDelay: "Maximum delay (ms)",
  totalLimit: "Total operation limit (ms)",
  activePreset: "Active preset",
  gainDb: "Gain {value} dB",
  highpass: "High-pass {value} Hz",
  denoise: "Denoise {value}%",
  punch: "Compression {value}%",
  importObs: "Import from OBS",
  obsMicMissing: "OBS microphone source was not found",
  unsupported: "Unsupported: {item}",
  sampleFailed: "Could not record a sample",
  micLevel: "Microphone level",
  meterLive: "This microphone is audible now.",
  meterIdle: "The meter will move when you start a sample recording.",
  peakRms: "Peak {peak}%, RMS {rms}%",
  clipping: ", clipping {count}",
  loadFailed: "Could not load settings",
  historyFailed: "Could not load history",
  settingsFailed: "Could not save settings",
  updateAvailable: "An update is available",
  updateNone: "No updates",
  updateBusy: "An update is already running",
  updateDeferred: "Update deferred until dictation finishes",
  updateFailed: "Could not check for updates",
  updateInstalled: "Update installed. Restarting…",
};

export type Messages = typeof ru;

export function messagesFor(locale: UiLocale): Messages {
  return locale === "en" ? en : ru;
}

export function messageKeys(locale: UiLocale): Array<keyof Messages> {
  return Object.keys(messagesFor(locale)) as Array<keyof Messages>;
}

export function localizedError(
  code: string | undefined,
  copy: Messages,
  fallback?: string,
): string {
  switch (code) {
    case "MicrophoneUnavailable":
      return copy.errMicrophone;
    case "AudioCaptureFailed":
      return copy.errAudioCapture;
    case "AudioProcessingFailed":
      return copy.errAudioProcessing;
    case "StorageFailed":
      return copy.errStorage;
    case "InvalidApiKey":
      return copy.errInvalidApiKey;
    case "InvalidModel":
      return copy.errInvalidModel;
    case "RequestValidationFailed":
      return copy.errRequestValidation;
    case "NetworkUnavailable":
      return copy.errNetwork;
    case "ConnectionFailed":
      return copy.errConnection;
    case "RequestTimeout":
      return copy.errTimeout;
    case "RateLimited":
      return copy.errRateLimited;
    case "ProviderUnavailable":
      return copy.errProvider;
    case "OpenRouterServerError":
      return copy.errServer;
    case "ResponseMalformed":
      return copy.errMalformed;
    case "RecordingTooLarge":
      return copy.errTooLarge;
    case "RetryDeadlineExceeded":
      return copy.errRetryDeadline;
    case "TextInsertionFailed":
      return copy.errInsert;
    case "Cancelled":
      return copy.errCancelled;
    case "HotkeyFailed":
      return copy.errHotkey;
    case "IllegalTransition":
      return copy.errIllegalTransition;
    case "TranscriptionInProgress":
      return copy.errTranscriptionInProgress;
    case "Interrupted":
      return copy.errInterrupted;
    default:
      return fallback?.trim() ? fallback : copy.errUnknown;
  }
}

export function formatInvokeError(error: unknown, copy: Messages): string {
  if (typeof error === "string") {
    return localizedError(error, copy, error);
  }
  if (error && typeof error === "object") {
    const record = error as { code?: string; message?: string; detail?: string };
    return localizedError(record.code, copy, record.message ?? record.detail);
  }
  return copy.errUnknown;
}

export function updateToast(code: string, copy: Messages): string {
  switch (code) {
    case "none":
      return copy.updateNone;
    case "installed":
      return copy.updateInstalled;
    case "busy":
      return copy.updateBusy;
    case "deferred":
      return copy.updateDeferred;
    default:
      return copy.updateFailed;
  }
}
