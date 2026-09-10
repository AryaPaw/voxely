import type { LucideIcon } from "lucide-react";
import {
  HardDrive,
  History,
  Keyboard,
  Languages,
  Mic,
  Palette,
  SlidersHorizontal,
  Wrench,
} from "lucide-react";
import { Button } from "../../components/ui/button";
import type { Messages } from "../../lib/i18n";

export type Section =
  | "history"
  | "general"
  | "audio"
  | "filters"
  | "transcription"
  | "historySettings"
  | "appearance"
  | "advanced";

export const SECTION_ICONS: Record<Section, LucideIcon> = {
  history: History,
  general: Keyboard,
  audio: Mic,
  filters: SlidersHorizontal,
  transcription: Languages,
  historySettings: HardDrive,
  appearance: Palette,
  advanced: Wrench,
};

export function sectionLabel(copy: Messages, id: Section): string {
  switch (id) {
    case "history":
      return copy.navHistory;
    case "general":
      return copy.navGeneral;
    case "audio":
      return copy.navAudio;
    case "filters":
      return copy.navFilters;
    case "transcription":
      return copy.navTranscription;
    case "historySettings":
      return copy.navStorage;
    case "appearance":
      return copy.navAppearance;
    case "advanced":
      return copy.navAdvanced;
    default: {
      const _never: never = id;
      return _never;
    }
  }
}

const SETTINGS_ITEMS: Section[] = [
  "general",
  "audio",
  "filters",
  "transcription",
  "historySettings",
  "appearance",
  "advanced",
];

export function SectionNav({
  current,
  copy,
  onSelect,
}: {
  current: Section;
  copy: Messages;
  onSelect: (section: Section) => void;
}) {
  return (
    <nav className="flex w-52 flex-col border-r border-border bg-panel p-3">
      <div className="mb-4 px-1 text-sm font-semibold tracking-tight">Voxely</div>
      <SectionButton id="history" current={current} copy={copy} onSelect={onSelect} />
      <div className="mt-4 mb-1 px-2 text-[11px] uppercase tracking-wide text-muted-foreground">
        {copy.settings}
      </div>
      {SETTINGS_ITEMS.map((id) => (
        <SectionButton key={id} id={id} current={current} copy={copy} onSelect={onSelect} />
      ))}
    </nav>
  );
}

function SectionButton({
  id,
  current,
  copy,
  onSelect,
}: {
  id: Section;
  current: Section;
  copy: Messages;
  onSelect: (section: Section) => void;
}) {
  const Icon = SECTION_ICONS[id];
  return (
    <Button
      type="button"
      variant={current === id ? "secondary" : "ghost"}
      className="w-full justify-start"
      aria-current={current === id ? "page" : undefined}
      onClick={() => onSelect(id)}
    >
      <Icon className="h-4 w-4" />
      {sectionLabel(copy, id)}
    </Button>
  );
}
