export type UiLocale = "ru" | "en";

export function resolveUiLocale(uiLanguage: string, navigatorLanguage = "en"): UiLocale {
  if (uiLanguage === "ru" || uiLanguage === "en") {
    return uiLanguage;
  }
  return navigatorLanguage.toLowerCase().startsWith("ru") ? "ru" : "en";
}

const ru = {
  navHistory: "История",
  navCompare: "Сравнение",
  navGeneral: "Общие",
  navAudio: "Микрофон",
  navFilters: "Фильтры",
  navTranscription: "Расшифровка",
  navStorage: "Хранение",
  navAppearance: "Внешний вид",
  navAdvanced: "Дополнительно",
  navDebug: "Песочница",
  navAbout: "О программе",
  compareIntro:
    "Один клип, несколько моделей OpenRouter рядом. Это не идёт в историю и не вставляется в другое окно.",
  compareRecord: "Записать клип",
  compareStop: "Стоп",
  compareRun: "Сравнить",
  compareAddModel: "Добавить модель",
  compareRemoveModel: "Убрать",
  compareMakeDefault: "Сделать основной",
  compareNoKey: "Сначала добавьте API-ключ на странице расшифровки.",
  compareNoClip: "Сначала запишите клип.",
  compareBusy: "Сначала остановите диктовку или запись фильтра.",
  compareSlotError: "Ошибка",
  compareAttempt: "Попытка {value}",
  debugCues: "Звуки диктовки",
  debugCueIntro: "Проверьте сигналы старта, конца записи и отмены. HUD при этом не открывается.",
  debugCueStart: "Старт",
  debugCueStop: "Конец записи",
  debugCueCancel: "Отмена",
  debugNotify: "Уведомления Windows",
  debugNotifyIntro:
    "Отправьте настоящее уведомление в систему. Это тот же путь, что у ошибок диктовки. HUD не открывается.",
  debugNotifySend: "Показать уведомление",
  historyTitle: "История",
  historyHint: "Диктовки хранятся локально на этом компьютере.",
  search: "Поиск по расшифровкам",
  folder: "Папка",
  settings: "Настройки",
  emptyHistory: "Записей пока нет. Нажмите {hotkey} в текстовом поле.",
  emptyTranscript: "Расшифровка пустая",
  cancelRetry: "Отменить повтор",
  noSearchResults: "Ничего не найдено по этому запросу.",
  clearSearch: "Очистить поиск",
  loadMore: "Показать ещё",
  settingsRecovered:
    "Файл настроек был повреждён. Показаны безопасные значения. Сохраните страницу, чтобы записать их, или восстановите копию settings.json.corrupt.",
  playbackFailed: "Не удалось воспроизвести запись.",
  processing: "Расшифровка",
  processingAudio: "Обработка",
  copy: "Копировать",
  retry: "Повторить расшифровку",
  copied: "Скопировано",
  durationSeconds: "сек.",
  durationMinutes: "мин.",
  filtersTitle: "Фильтры",
  filtersIntro:
    "Запишите короткую фразу на этой странице, включите нужные фильтры и сравните две кнопки. По умолчанию включены только лёгкий срез низов и чуть громкости, поэтому разница едва слышна. Чтобы услышать шум и динамику, включите шумоподавление и компрессор. Обе кнопки используют одну громкость прослушивания, поэтому усиление и компрессор остаются слышны. Это тот же звук, что в истории, без сжатия для расшифровки.",
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
  installUpdate: "Установить обновление",
  installing: "Установка…",
  checking: "Проверка…",
  startWithWindows: "Запускать вместе с Windows",
  closeToTray: "Сворачивать в трей",
  notifications: "Уведомления Windows",
  overlayReady: "Готово",
  overlayRecording: "Запись",
  overlayRecordingLimit: "Лимит записи",
  overlaySaving: "Сохранение",
  overlayProcessing: "Обработка",
  overlayTranscribing: "Расшифровка",
  overlayRetry: "Расшифровка (try {attempt})",
  overlayWaiting: "Ожидание",
  overlayError: "Ошибка",
  overlayCancel: "Отменить запись",
  overlayCancelAria: "Наведите, чтобы отменить запись",
  loading: "Загрузка…",
  retryAction: "Повторить",
  defaultMic: "Системное по умолчанию",
  deviceUnavailable: "недоступен",
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
  latencyMs: "{n} мс",
  modelsFound: "Доступно моделей: {n}",
  addApiKey: "Добавить ключ",
  missingApiKeyBanner: "Нет API-ключа OpenRouter. Запись сохранится, расшифровка не отправится.",
  details: "Сведения",
  delete: "Удалить",
  transcriptUnavailable: "Расшифровка недоступна.",
  pause: "Пауза",
  stopListen: "Стоп",
  listen: "Слушать",
  modelLabel: "Модель",
  customModel: "Свой идентификатор модели",
  catalogUnavailable: "Каталог моделей сейчас недоступен. Можно ввести идентификатор вручную.",
  modelRequired: "Укажите модель",
  latencyLabel: "Задержка",
  costLabel: "Стоимость",
  errorLabel: "Ошибка",
  successLabel: "Успех",
  globalHotkey: "Глобальный хоткей",
  hotkeyPrompt: "Нажмите сочетание… Esc отмена",
  inputDevice: "Устройство ввода",
  theme: "Тема",
  themeSystem: "Системная",
  themeLight: "Светлая",
  themeDark: "Тёмная",
  insertMode: "Вставка текста",
  insertAuto: "Авто",
  insertUnicode: "В окно (Unicode)",
  insertSendInput: "SendInput",
  insertClipboard: "Только буфер обмена",
  insertHint:
    "Unicode вставляет расшифровку в окно, где вы говорили. «Только буфер» копирует текст и не вставляет его: вставьте сами.",
  copiedInsert: "Текст скопирован, вставьте вручную",
  copiedPartial: "Вставка прервана, часть текста могла вставиться; полный текст скопирован",
  debugLogs: "Отладочные логи",
  openLogs: "Открыть логи",
  openSettingsFolder: "Открыть папку настроек",
  openLogsFailed: "Не удалось открыть папку логов",
  openSettingsFailed: "Не удалось открыть папку настроек",
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
  keepOriginals: "Хранить исходные записи (для повторной обработки)",
  deleteAllHistory: "Удалить всю историю",
  deleteAllConfirm: "Удалить всю историю?",
  deleteAllCannotUndo: "Это нельзя отменить.",
  deleteItemConfirm: "Удалить эту запись?",
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
  language: "Язык речи",
  speechLanguageAuto: "Автоопределение",
  speechLanguageHint:
    "Язык того, что вы говорите в микрофон. Автоопределение обычно достаточно. Зафиксируйте язык, если почти всегда диктуете на одном.",
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
  unsupported: "Не поддерживается: {item}",
  sampleFailed: "Не удалось записать образец",
  micLevel: "Уровень микрофона",
  meterLive: "Сейчас слышно этот микрофон.",
  meterIdle: "Полоска оживёт, когда начнёте запись образца.",
  peakRms: "Пик {peak} dBFS, RMS {rms}",
  clipping: ", клиппинг {count}",
  tooQuiet: "Слишком тихо",
  clippingWarning: "Клиппинг",
  noFilterSample: "Сначала запишите образец на этой странице.",
  filterRnnoise: "Шумоподавление",
  filterRnnoiseMix: "Смесь шумоподавления {value}%",
  filterCompressor: "Компрессор",
  filterExpander: "Экспандер",
  filterGate: "Гейт",
  filterLimiter: "Лимитер",
  presetSttFast: "Быстрая диктовка",
  presetSttOptimized: "Качество (медленнее)",
  presetObsImported: "Импорт из OBS",
  resetSettings: "Сбросить настройки",
  resetSettingsConfirm: "Сбросить настройки?",
  resetSettingsHint: "Вернутся значения по умолчанию. API-ключ не трогаем. История не удаляется.",
  resetAll: "Полный сброс",
  resetAllConfirm: "Полностью сбросить приложение?",
  resetAllHint: "Сбросятся все настройки и API-ключ. История не удаляется.",
  resetDone: "Настройки сброшены",
  resetAllDone: "Настройки и API-ключ сброшены",
  aboutTitle: "О программе",
  aboutAuthor: "Автор",
  aboutVersion: "Версия",
  aboutTagline: "Диктовка в любое текстовое поле",
  aboutSource: "Исходный код",
  aboutReport: "Сообщить о проблеме",
  aboutReportHint: "Откроется GitHub Issues",
  authorName: "AryaPaw",
  appName: "Voxely",
  appNameLocal: "Voxely (локальная)",
  githubRepo: "AryaPaw/voxely",
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
  navHistory: "History",
  navCompare: "Compare",
  navGeneral: "General",
  navAudio: "Microphone",
  navFilters: "Filters",
  navTranscription: "Transcription",
  navStorage: "Storage",
  navAppearance: "Appearance",
  navAdvanced: "Advanced",
  navDebug: "Sandbox",
  navAbout: "About",
  compareIntro:
    "One clip, several OpenRouter models side by side. This does not go into history and is not inserted into another window.",
  compareRecord: "Record clip",
  compareStop: "Stop",
  compareRun: "Compare",
  compareAddModel: "Add model",
  compareRemoveModel: "Remove",
  compareMakeDefault: "Set as default",
  compareNoKey: "Add an API key on the transcription page first.",
  compareNoClip: "Record a clip first.",
  compareBusy: "Stop dictation or the filter sample first.",
  compareSlotError: "Error",
  compareAttempt: "Attempt {value}",
  debugCues: "Dictation cues",
  debugCueIntro: "Play the start, stop, and cancel cues. This does not open the HUD.",
  debugCueStart: "Start",
  debugCueStop: "End of recording",
  debugCueCancel: "Cancel",
  debugNotify: "Windows notifications",
  debugNotifyIntro:
    "Send a real Windows toast. This is the same path as dictation errors. The HUD does not open.",
  debugNotifySend: "Show notification",
  historyTitle: "History",
  historyHint: "Dictations stay on this computer.",
  search: "Search transcripts",
  folder: "Folder",
  settings: "Settings",
  emptyHistory: "No recordings yet. Press {hotkey} in a text field.",
  emptyTranscript: "Transcript is empty",
  cancelRetry: "Cancel retry",
  noSearchResults: "No results for this search.",
  clearSearch: "Clear search",
  loadMore: "Load more",
  settingsRecovered:
    "Settings were damaged. Safe defaults are shown. Save to write them, or restore settings.json.corrupt.",
  playbackFailed: "Playback failed.",
  processing: "Transcribing",
  processingAudio: "Processing",
  copy: "Copy",
  retry: "Retry transcription",
  copied: "Copied",
  durationSeconds: "sec.",
  durationMinutes: "min.",
  filtersTitle: "Filters",
  filtersIntro:
    "Record a short phrase here, enable the filters you want, then compare the two buttons. The default preset is only a light low cut and a little gain, so the difference is subtle. Turn on noise reduction and compressor to hear a clearer change. Both buttons share one listen gain, so extra gain and compression stay audible. This is the same audio as History, not the downsampled STT file.",
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
  installUpdate: "Install update",
  installing: "Installing…",
  checking: "Checking…",
  startWithWindows: "Start with Windows",
  closeToTray: "Minimize to tray",
  notifications: "Windows notifications",
  overlayReady: "Ready",
  overlayRecording: "Recording",
  overlayRecordingLimit: "Recording limit",
  overlaySaving: "Saving",
  overlayProcessing: "Processing",
  overlayTranscribing: "Transcribing",
  overlayRetry: "Transcribing (try {attempt})",
  overlayWaiting: "Waiting",
  overlayError: "Error",
  overlayCancel: "Cancel recording",
  overlayCancelAria: "Hover to cancel recording",
  loading: "Loading…",
  retryAction: "Retry",
  defaultMic: "System default",
  deviceUnavailable: "unavailable",
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
  secondsAbbrev: "sec",
  latencyMs: "{n} ms",
  modelsFound: "{n} models available",
  addApiKey: "Add key",
  missingApiKeyBanner:
    "OpenRouter API key is missing. The recording is saved, transcription is skipped.",
  details: "Details",
  delete: "Delete",
  transcriptUnavailable: "Transcription is unavailable.",
  pause: "Pause",
  stopListen: "Stop",
  listen: "Listen",
  modelLabel: "Model",
  customModel: "Custom model id",
  catalogUnavailable: "The model catalog is unavailable. You can type an id manually.",
  modelRequired: "Enter a model",
  latencyLabel: "Latency",
  costLabel: "Cost",
  errorLabel: "Error",
  successLabel: "Success",
  globalHotkey: "Global hotkey",
  hotkeyPrompt: "Press a shortcut… Esc cancels",
  inputDevice: "Input device",
  theme: "Theme",
  themeSystem: "System",
  themeLight: "Light",
  themeDark: "Dark",
  insertMode: "Text insertion",
  insertAuto: "Auto",
  insertUnicode: "Into window (Unicode)",
  insertSendInput: "SendInput",
  insertClipboard: "Clipboard only",
  insertHint:
    "Unicode inserts the transcript into the window where you spoke. Clipboard only copies the text; paste it yourself.",
  copiedInsert: "Text copied. Paste it yourself.",
  copiedPartial:
    "Insert interrupted. Some text may already be in the field. Full transcript copied.",
  debugLogs: "Debug logs",
  openLogs: "Open logs",
  openSettingsFolder: "Open settings folder",
  openLogsFailed: "Could not open the logs folder",
  openSettingsFailed: "Could not open the settings folder",
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
  keepOriginals: "Keep original recordings (for reprocessing)",
  deleteAllHistory: "Delete all history",
  deleteAllConfirm: "Delete all history?",
  deleteAllCannotUndo: "This cannot be undone.",
  deleteItemConfirm: "Delete this recording?",
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
  language: "Speech language",
  speechLanguageAuto: "Auto-detect",
  speechLanguageHint:
    "Language of what you speak into the microphone. Auto-detect is usually enough. Pin a language if you almost always dictate in one.",
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
  unsupported: "Unsupported: {item}",
  sampleFailed: "Could not record a sample",
  micLevel: "Microphone level",
  meterLive: "This microphone is audible now.",
  meterIdle: "The meter will move when you start a sample recording.",
  peakRms: "Peak {peak} dBFS, RMS {rms}",
  clipping: ", clipping {count}",
  tooQuiet: "Too quiet",
  clippingWarning: "Clipping",
  noFilterSample: "Record a sample on this page first.",
  filterRnnoise: "Noise reduction",
  filterRnnoiseMix: "Noise reduction mix {value}%",
  filterCompressor: "Compressor",
  filterExpander: "Expander",
  filterGate: "Gate",
  filterLimiter: "Limiter",
  presetSttFast: "Fast dictation",
  presetSttOptimized: "Quality (slower)",
  presetObsImported: "OBS imported",
  resetSettings: "Reset settings",
  resetSettingsConfirm: "Reset settings?",
  resetSettingsHint:
    "Factory defaults will be restored. The API key stays. History is not deleted.",
  resetAll: "Reset everything",
  resetAllConfirm: "Reset the whole app?",
  resetAllHint: "All settings and the API key will be cleared. History is not deleted.",
  resetDone: "Settings were reset",
  resetAllDone: "Settings and API key were reset",
  aboutTitle: "About",
  aboutAuthor: "Author",
  aboutVersion: "Version",
  aboutTagline: "Voice dictation into any text field",
  aboutSource: "Source code",
  aboutReport: "Report a problem",
  aboutReportHint: "Opens GitHub Issues",
  authorName: "AryaPaw",
  appName: "Voxely",
  appNameLocal: "Voxely (local)",
  githubRepo: "AryaPaw/voxely",
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

export function applyUiLocale(locale: UiLocale): void {
  document.documentElement.lang = locale;
}

export type Messages = typeof ru;

export function appDisplayName(copy: Messages): string {
  return copy.appName;
}

export function factoryPresetLabel(preset: { id: string; name: string }, copy: Messages): string {
  switch (preset.id) {
    case "stt-fast":
      return copy.presetSttFast;
    case "stt-optimized":
      return copy.presetSttOptimized;
    case "obs-imported":
      return copy.presetObsImported;
    default:
      return preset.name;
  }
}

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

export function statusToast(kind: "error" | "success", copy: Messages, message: string): string {
  return `${kind === "error" ? copy.errorLabel : copy.successLabel}: ${message}`;
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
    case "available":
      return copy.updateAvailable;
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
