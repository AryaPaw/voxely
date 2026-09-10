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
};

export type Messages = typeof ru;

export function messagesFor(locale: UiLocale): Messages {
  return locale === "en" ? en : ru;
}
