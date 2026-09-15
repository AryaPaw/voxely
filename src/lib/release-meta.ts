export function formatReleaseDate(isoDay: string, locale: string): string {
  const date = new Date(`${isoDay}T00:00:00Z`);
  return new Intl.DateTimeFormat(locale, {
    day: "numeric",
    month: "short",
    year: "numeric",
    timeZone: "UTC",
  }).format(date);
}

export function versionWithReleaseDate(version: string, isoDay: string, locale: string): string {
  if (!version) {
    return "…";
  }
  return `${version} (${formatReleaseDate(isoDay, locale)})`;
}
