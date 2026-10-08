import { useState } from "react";
import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  arrayMove,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { CaseSensitive, GripVertical, Plus, Trash2 } from "lucide-react";
import type { AppSettings, TextReplacementRule } from "../../../lib/api";
import type { Messages } from "../../../lib/i18n";
import russianFormalRules from "../../../lib/text-replacement-presets.json";
import { Button } from "../../../components/ui/button";
import { Input } from "../../../components/ui/input";
import { Switch } from "../../../components/ui/switch";
import { Tooltip, TooltipContent, TooltipTrigger } from "../../../components/ui/tooltip";

const RULE_LIMIT = 100;

export function TextReplacementSettings({
  settings,
  copy,
  onChange,
}: {
  settings: AppSettings;
  copy: Messages;
  onChange: (patch: Partial<AppSettings>) => void;
}) {
  const textSettings = settings.textReplacements;
  // UI identities survive edits and moves without changing the persisted rule schema
  const [ruleIds, setRuleIds] = useState<string[]>(() =>
    textSettings.rules.map(() => crypto.randomUUID()),
  );
  if (ruleIds.length !== textSettings.rules.length) {
    setRuleIds(textSettings.rules.map((_, index) => ruleIds[index] ?? crypto.randomUUID()));
  }
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );
  const knownRules = new Set(textSettings.rules.map((rule) => rule.from.toLowerCase()));
  const presetAdditions = russianFormalRules.filter(
    (rule) => !knownRules.has(rule.from.toLowerCase()),
  );

  function update(next: Partial<typeof textSettings>) {
    onChange({ textReplacements: { ...textSettings, ...next } });
  }

  function updateRule(index: number, patch: Partial<TextReplacementRule>) {
    update({
      rules: textSettings.rules.map((rule, i) => (i === index ? { ...rule, ...patch } : rule)),
    });
  }

  function onDragEnd({ active, over }: DragEndEvent) {
    if (!over || active.id === over.id) return;
    const from = ruleIds.indexOf(String(active.id));
    const to = ruleIds.indexOf(String(over.id));
    if (from < 0 || to < 0) return;
    setRuleIds(arrayMove(ruleIds, from, to));
    update({ rules: arrayMove(textSettings.rules, from, to) });
  }

  return (
    <section
      aria-labelledby="text-replacements-heading"
      className="overflow-hidden rounded-xl border border-border bg-card"
    >
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border px-4 py-3">
        <div className="flex items-center gap-2.5">
          <Switch
            id="text-replacements-enabled"
            aria-label={copy.textReplacementsEnabled}
            checked={textSettings.enabled}
            onCheckedChange={(enabled) => update({ enabled })}
          />
          <h2 id="text-replacements-heading" className="text-sm font-medium">
            <label htmlFor="text-replacements-enabled" className="cursor-pointer">
              {copy.textReplacementsTitle}
            </label>
          </h2>
          <span className="text-xs tabular-nums text-muted-foreground">
            {textSettings.rules.length} / {RULE_LIMIT}
          </span>
        </div>
        <div className="flex items-center gap-2">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="outline"
                size="sm"
                aria-label={copy.textReplacementRussianPreset}
                disabled={
                  presetAdditions.length === 0 ||
                  textSettings.rules.length + presetAdditions.length > RULE_LIMIT
                }
                onClick={() =>
                  update({ enabled: true, rules: [...textSettings.rules, ...presetAdditions] })
                }
              >
                {copy.textReplacementRussianPreset}
              </Button>
            </TooltipTrigger>
            <TooltipContent>{copy.textReplacementPresetHint}</TooltipContent>
          </Tooltip>
          <Button
            type="button"
            size="sm"
            disabled={textSettings.rules.length >= RULE_LIMIT}
            onClick={() =>
              update({ rules: [...textSettings.rules, { from: "", to: "", caseSensitive: false }] })
            }
          >
            <Plus />
            {copy.textReplacementAddRule}
          </Button>
        </div>
      </div>
      {textSettings.rules.length === 0 ? (
        <p className="px-4 py-8 text-center text-sm text-muted-foreground">
          {copy.textReplacementEmpty}
        </p>
      ) : (
        <>
          <div
            aria-hidden="true"
            className="grid grid-cols-[1.75rem_minmax(0,1fr)_minmax(0,1fr)_1.75rem_1.75rem] gap-2 px-3 pb-1 pt-3 text-xs text-muted-foreground"
          >
            <span /> <span>{copy.textReplacementFindLabel}</span>
            <span>{copy.textReplacementReplaceLabel}</span>
          </div>
          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            onDragEnd={onDragEnd}
            accessibility={{
              screenReaderInstructions: { draggable: copy.textReplacementDragInstructions },
              announcements: {
                onDragStart: ({ active }) =>
                  copy.textReplacementDragPicked.replace(
                    "{index}",
                    String(ruleIds.indexOf(String(active.id)) + 1),
                  ),
                onDragOver: ({ over }) =>
                  over
                    ? copy.textReplacementDragPosition.replace(
                        "{index}",
                        String(ruleIds.indexOf(String(over.id)) + 1),
                      )
                    : undefined,
                onDragEnd: ({ over }) =>
                  over ? copy.textReplacementDragDone : copy.textReplacementDragCancelled,
                onDragCancel: () => copy.textReplacementDragCancelled,
              },
            }}
          >
            <SortableContext items={ruleIds} strategy={verticalListSortingStrategy}>
              <ol className="space-y-1 px-2 pb-2">
                {textSettings.rules.map((rule, index) => (
                  <ReplacementRow
                    key={ruleIds[index]}
                    id={ruleIds[index]}
                    rule={rule}
                    index={index}
                    copy={copy}
                    onChange={(patch) => updateRule(index, patch)}
                    onRemove={() => {
                      setRuleIds(ruleIds.filter((_, i) => i !== index));
                      update({ rules: textSettings.rules.filter((_, i) => i !== index) });
                    }}
                  />
                ))}
              </ol>
            </SortableContext>
          </DndContext>
        </>
      )}
      <p className="border-t border-border px-4 py-2 text-xs text-muted-foreground">
        {copy.textReplacementsIntro}
      </p>
    </section>
  );
}

function ReplacementRow({
  id,
  rule,
  index,
  copy,
  onChange,
  onRemove,
}: {
  id: string;
  rule: TextReplacementRule;
  index: number;
  copy: Messages;
  onChange: (patch: Partial<TextReplacementRule>) => void;
  onRemove: () => void;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    setActivatorNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id });
  return (
    <li
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      className={`relative grid grid-cols-[1.75rem_minmax(0,1fr)_minmax(0,1fr)_1.75rem_1.75rem] items-center gap-2 rounded-lg p-1 transition-colors hover:bg-muted/50 ${isDragging ? "z-10 bg-muted shadow-md" : ""}`}
    >
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        ref={setActivatorNodeRef}
        className="touch-none cursor-grab text-muted-foreground active:cursor-grabbing"
        aria-label={copy.textReplacementDrag.replace("{index}", String(index + 1))}
        {...attributes}
        {...listeners}
      >
        <GripVertical />
      </Button>
      <Input
        aria-label={copy.textReplacementFrom.replace("{index}", String(index + 1))}
        placeholder={copy.textReplacementFindLabel}
        maxLength={200}
        autoComplete="off"
        spellCheck={false}
        value={rule.from}
        onChange={(event) => onChange({ from: event.target.value })}
      />
      <Input
        aria-label={copy.textReplacementTo.replace("{index}", String(index + 1))}
        placeholder={copy.textReplacementReplaceLabel}
        maxLength={200}
        autoComplete="off"
        spellCheck={false}
        value={rule.to}
        onChange={(event) => onChange({ to: event.target.value })}
      />
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            type="button"
            variant={rule.caseSensitive ? "secondary" : "ghost"}
            size="icon-sm"
            className={rule.caseSensitive ? "text-primary" : "text-muted-foreground"}
            aria-label={`${copy.textReplacementCaseSensitive}, ${index + 1}`}
            aria-pressed={rule.caseSensitive}
            onClick={() => onChange({ caseSensitive: !rule.caseSensitive })}
          >
            <CaseSensitive />
          </Button>
        </TooltipTrigger>
        <TooltipContent>{copy.textReplacementCaseSensitive}</TooltipContent>
      </Tooltip>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        className="text-muted-foreground hover:text-destructive"
        aria-label={copy.textReplacementRemove.replace("{index}", String(index + 1))}
        onClick={onRemove}
      >
        <Trash2 />
      </Button>
    </li>
  );
}
