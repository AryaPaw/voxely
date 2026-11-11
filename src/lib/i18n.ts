import appIdentity from "./app-identity.json";
import errorKeys from "./locales/errors.json";
import ru from "./locales/ru.json";
import en from "./locales/en.json";
export type UiLocale = "ru" | "en";

export function resolveUiLocale(uiLanguage: string, navigatorLanguage = "en"): UiLocale {
  if (uiLanguage === "ru" || uiLanguage === "en") {
    return uiLanguage;
  }
  return navigatorLanguage.toLowerCase().startsWith("ru") ? "ru" : "en";
}

export function applyUiLocale(locale: UiLocale): void {
  document.documentElement.lang = locale;
}

export type Messages = typeof ru;

export function appDisplayName(_copy: Messages, localBuild = false): string {
  return localBuild ? appIdentity.localBuildName : appIdentity.name;
}

export function factoryPresetLabel(preset: { id: string; name: string }, copy: Messages): string {
  switch (preset.id) {
    case "stt-fast":
      return copy.presetSttFast;
    case "stt-optimized":
      return copy.presetSttOptimized;
    default:
      return preset.name;
  }
}

export function messagesFor(locale: UiLocale): Messages {
  return locale === "en" ? en : ru;
}

export function messagesForUiLanguage(uiLanguage: string, systemLanguage = "en"): Messages {
  return messagesFor(resolveUiLocale(uiLanguage, systemLanguage));
}

export function messageKeys(locale: UiLocale): Array<keyof Messages> {
  return Object.keys(messagesFor(locale)) as Array<keyof Messages>;
}

export function localizedError(
  code: string | undefined,
  copy: Messages,
  fallback?: string,
): string {
  if (
    code === "RecordingTooLarge" &&
    /truncated|maximum (?:recording )?size/i.test(fallback ?? "")
  ) {
    return copy.errTruncated;
  }
  const key = code ? errorKeys[code as keyof typeof errorKeys] : undefined;
  if (key && key in copy) return copy[key as keyof Messages];
  return fallback?.trim() ? fallback : copy.errUnknown;
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
