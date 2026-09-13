export { cn } from "cn";

export type DurationUnits = {
  seconds: string;
  minutes: string;
};

const RUSSIAN_DURATION: DurationUnits = {
  seconds: "сек.",
  minutes: "мин.",
};

export function formatDuration(ms: number, units: DurationUnits = RUSSIAN_DURATION): string {
  const total = Math.max(0, Math.round(ms / 1000));
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  if (minutes === 0) {
    return `${total} ${units.seconds}`;
  }
  if (seconds === 0) {
    return `${minutes} ${units.minutes}`;
  }
  return `${minutes} ${units.minutes} ${seconds} ${units.seconds}`;
}

export function formatTime(iso: string, locale = "ru-RU"): string {
  const date = new Date(iso);
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}
