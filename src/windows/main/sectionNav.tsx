import type { LucideIcon } from "lucide-react";
import {
  Bug,
  Columns2,
  ChartNoAxesCombined,
  HardDrive,
  History,
  Info,
  Settings2,
  Languages,
  Mic,
  Palette,
  ReplaceAll,
  Wrench,
} from "lucide-react";
import { Button } from "../../components/ui/button";
import type { Messages } from "../../lib/i18n";

export type Section =
  | "history"
  | "statistics"
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
  statistics: ChartNoAxesCombined,
  compare: Columns2,
  general: Settings2,
  audio: Mic,
  filters: ReplaceAll,
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
    case "statistics":
      return copy.navStatistics;
    case "compare":
      return copy.navCompare;
    case "general":
      return copy.navGeneral;
    case "audio":
      return copy.navAudio;
    case "filters":
      return copy.textReplacementsTitle;
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

const SETTINGS_GROUPS: { label: keyof Messages; ids: Section[] }[] = [
  { label: "navDictationGroup", ids: ["audio", "transcription", "filters"] },
  { label: "navApplicationGroup", ids: ["general", "historySettings", "advanced"] },
];

export function settingsNavItems(localBuild: boolean): Section[] {
  return [
    ...SETTINGS_GROUPS.flatMap((group) => group.ids),
    ...(localBuild ? ["debug" as const] : []),
    "about",
  ];
}

export function SectionNav({
  current,
  copy,
  localBuild,
  onSelect,
}: {
  current: Section;
  copy: Messages;
  localBuild: boolean;
  onSelect: (section: Section) => void;
}) {
  return (
    <nav className="section-nav" aria-label={copy.settings}>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="nav-workspace">
          <SectionButton id="history" current={current} copy={copy} onSelect={onSelect} />
          <SectionButton id="statistics" current={current} copy={copy} onSelect={onSelect} />
          <SectionButton id="compare" current={current} copy={copy} onSelect={onSelect} />
        </div>
        {SETTINGS_GROUPS.map((group) => (
          <section className="nav-group" key={group.label} aria-label={copy[group.label]}>
            <h2>{copy[group.label]}</h2>
            {group.ids.map((id) => (
              <SectionButton key={id} id={id} current={current} copy={copy} onSelect={onSelect} />
            ))}
          </section>
        ))}
      </div>
      <div className="nav-footer">
        {localBuild ? (
          <SectionButton id="debug" current={current} copy={copy} onSelect={onSelect} />
        ) : null}
        <SectionButton id="about" current={current} copy={copy} onSelect={onSelect} />
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
      variant="ghost"
      className="section-nav-button"
      aria-label={sectionLabel(copy, id)}
      aria-current={current === id ? "page" : undefined}
      onClick={() => onSelect(id)}
    >
      <Icon className="h-4 w-4" />
      <span className="hidden sm:inline">{sectionLabel(copy, id)}</span>
    </Button>
  );
}
