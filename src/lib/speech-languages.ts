import languages from "./locales/speech-languages.json";
export interface SpeechLanguage {
  code: string;
  nameEn: string;
  native: string;
}

export const SPEECH_LANGUAGES: SpeechLanguage[] = languages;

export function speechLanguageOptions(
  autoLabel: string,
  locale: string,
  current?: string,
): Array<{ value: string; label: string }> {
  const named = SPEECH_LANGUAGES.map((item) => ({
    value: item.code,
    label: locale.startsWith("ru")
      ? `${item.native} (${item.code})`
      : `${item.nameEn} (${item.code})`,
  })).sort((a, b) => a.label.localeCompare(b.label, locale, { sensitivity: "base" }));
  const options = [{ value: "auto", label: autoLabel }, ...named];
  if (current && current !== "auto" && !options.some((item) => item.value === current)) {
    options.push({ value: current, label: current });
  }
  return options;
}
