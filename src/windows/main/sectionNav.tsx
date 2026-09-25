import type { LucideIcon } from "lucide-react";
import {
  Bug,
  Columns2,
  HardDrive,
  History,
  Info,
  Keyboard,
  Languages,
  Mic,
  Palette,
  SlidersHorizontal,
  Wrench,
} from "lucide-react";
import { Button } from "../../components/ui/button";
import { appDisplayName, type Messages } from "../../lib/i18n";

export type Section =
  | "history"
  | "compare"
  | "general"
  | "audio"
  | "filters"
  | "transcription"
  | "historySettings"
  | "appearance"
  | "advanced"
  | "debug"
  | "about";

export const SECTION_ICONS: Record<Section, LucideIcon> = {
  history: History,
  compare: Columns2,
  general: Keyboard,
  audio: Mic,
  filters: SlidersHorizontal,
  transcription: Languages,
  historySettings: HardDrive,
  appearance: Palette,
  advanced: Wrench,
  debug: Bug,
  about: Info,
};

export function sectionLabel(copy: Messages, id: Section): string {
  switch (id) {
    case "history":
      return copy.navHistory;
    case "compare":
      return copy.navCompare;
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
    case "debug":
      return copy.navDebug;
    case "about":
      return copy.navAbout;
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
  "debug",
  "about",
];

export function settingsNavItems(localBuild: boolean): Section[] {
  return SETTINGS_ITEMS.filter((id) => id !== "debug" || localBuild);
}

export function SectionNav({
  current,
  copy,
  localBuild,
  version,
  onSelect,
}: {
  current: Section;
  copy: Messages;
  localBuild: boolean;
  version: string;
  onSelect: (section: Section) => void;
}) {
  return (
    <nav className="flex h-full w-16 flex-col border-r border-border bg-panel p-2 sm:w-56 sm:p-3">
      <div className="mb-4 flex items-center gap-2 px-1">
        <img src="/favicon.png" alt="" className="h-9 w-9 rounded-lg sm:h-10 sm:w-10" />
        <div className="hidden min-w-0 sm:block">
          <div className="text-sm font-semibold tracking-tight">{appDisplayName(copy)}</div>
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <SectionButton id="history" current={current} copy={copy} onSelect={onSelect} />
        <SectionButton id="compare" current={current} copy={copy} onSelect={onSelect} />
        <div className="mt-4 mb-1 hidden px-2 text-[11px] uppercase tracking-wide text-muted-foreground sm:block">
          {copy.settings}
        </div>
        {settingsNavItems(localBuild).map((id) => (
          <SectionButton key={id} id={id} current={current} copy={copy} onSelect={onSelect} />
        ))}
      </div>
      <div className="mt-3 px-1 text-center text-[11px] tabular-nums text-muted-foreground sm:text-left">
        {version ? (
          <span>
            {version}
            {localBuild ? ` (${copy.navLocalBuild})` : ""}
          </span>
        ) : null}
      </div>
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
      className="w-full justify-center sm:justify-start"
      aria-label={sectionLabel(copy, id)}
      aria-current={current === id ? "page" : undefined}
      onClick={() => onSelect(id)}
    >
      <Icon className="h-4 w-4" />
      <span className="hidden sm:inline">{sectionLabel(copy, id)}</span>
    </Button>
  );
}
