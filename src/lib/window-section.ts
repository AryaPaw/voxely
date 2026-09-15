import type { Section } from "../windows/main/sectionNav";

export const APP_NAVIGATE = "app://navigate";

const SECTIONS: Section[] = [
  "history",
  "compare",
  "general",
  "audio",
  "filters",
  "transcription",
  "historySettings",
  "appearance",
  "advanced",
  "debug",
  "about",
];

function isSection(value: string): value is Section {
  return (SECTIONS as string[]).includes(value);
}

export function sectionFromSearch(search: string, localBuild = false): Section {
  const query = search.startsWith("?") ? search.slice(1) : search;
  const value = new URLSearchParams(query).get("section");
  if (value === "debug" && !localBuild) {
    return "history";
  }
  if (value && isSection(value)) {
    return value;
  }
  return "history";
}

export function sectionFromNavigatePayload(payload: string, localBuild = false): Section {
  return sectionFromSearch(`?section=${encodeURIComponent(payload)}`, localBuild);
}
