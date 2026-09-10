import type { Section } from "../windows/main/sectionNav";

const SECTIONS: Section[] = [
  "history",
  "general",
  "audio",
  "filters",
  "transcription",
  "historySettings",
  "appearance",
  "advanced",
  "about",
];

function isSection(value: string): value is Section {
  return (SECTIONS as string[]).includes(value);
}

export function sectionFromSearch(search: string): Section {
  const query = search.startsWith("?") ? search.slice(1) : search;
  const value = new URLSearchParams(query).get("section");
  if (value && isSection(value)) {
    return value;
  }
  return "history";
}
