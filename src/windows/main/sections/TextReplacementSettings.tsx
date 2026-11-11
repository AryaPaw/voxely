import { useEffect, useState } from "react";
import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  SortableContext,
  arrayMove,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import {
  CaseSensitive,
  ChevronDown,
  ChevronUp,
  Copy,
  GripVertical,
  Plus,
  Trash2,
} from "lucide-react";
import type { AppSettings, ReplacementGroup, TextReplacementRule } from "../../../lib/api";
import type { Messages } from "../../../lib/i18n";
import russianFormalRules from "../../../lib/text-replacement-presets.json";
import { Button } from "../../../components/ui/button";
import { Input } from "../../../components/ui/input";
import { Switch } from "../../../components/ui/switch";
import { Tooltip, TooltipContent, TooltipTrigger } from "../../../components/ui/tooltip";

import { PageHeader } from "../../../components/settings/PageHeader";
import { SECTION_ICONS } from "../sectionNav";
import {
  AlertDialog,
  AlertDialogTrigger,
  AlertDialogContent,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogCancel,
  AlertDialogAction,
} from "../../../components/ui/alert-dialog";

const RULE_LIMIT = 100;
const GROUP_LIMIT = 20;

export function TextReplacementSettings({
  settings,
  copy,
  onChange,
}: {
  settings: AppSettings;
  copy: Messages;
  onChange: (patch: Partial<AppSettings>) => void;
}) {
  const value = settings.textReplacements;
  const groups = value.groups;
  const customGroupCount = groups.filter((group) => group.presetId === null).length;
  const count = groups.reduce((sum, group) => sum + (group.rules?.length ?? 0), 0);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );
  function update(next: ReplacementGroup[]) {
    onChange({ textReplacements: { ...value, groups: next } });
  }
  function addGroup() {
    if (customGroupCount >= GROUP_LIMIT) return;
    update([
      ...groups,
      {
        id: crypto.randomUUID(),
        name: copy.replacementCustomName,
        enabled: true,
        presetId: null,
        rules: [],
      },
    ]);
  }
  return (
    <div>
      <PageHeader icon={SECTION_ICONS.filters} title={copy.textReplacementsTitle} />
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <label className="flex items-center gap-2 text-sm">
          <Switch
            checked={value.enabled}
            onCheckedChange={(enabled) => onChange({ textReplacements: { ...value, enabled } })}
            aria-label={copy.textReplacementsEnabled}
          />
          {copy.textReplacementsEnabled}
        </label>
        <Button size="sm" onClick={addGroup} disabled={customGroupCount >= GROUP_LIMIT}>
          <Plus />
          {copy.replacementAddGroup}
        </Button>
      </div>
      <DndContext
        accessibility={{
          screenReaderInstructions: { draggable: copy.replacementGroupInstructions },
          announcements: {
            onDragStart: ({ active }) =>
              copy.replacementGroupPicked.replace(
                "{index}",
                String(groups.findIndex((group) => group.id === active.id) + 1),
              ),
            onDragOver: ({ over }) =>
              over
                ? copy.replacementGroupPosition.replace(
                    "{index}",
                    String(groups.findIndex((group) => group.id === over.id) + 1),
                  )
                : undefined,
            onDragEnd: ({ over }) =>
              over ? copy.replacementGroupDone : copy.replacementGroupCancelled,
            onDragCancel: () => copy.replacementGroupCancelled,
          },
        }}
        sensors={sensors}
        collisionDetection={closestCenter}
        onDragEnd={({ active, over }) => {
          if (!over || active.id === over.id) return;
          const from = groups.findIndex((group) => group.id === active.id);
          const to = groups.findIndex((group) => group.id === over.id);
          if (from >= 0 && to >= 0) update(arrayMove(groups, from, to));
        }}
      >
        <SortableContext
          items={groups.map((group) => group.id)}
          strategy={verticalListSortingStrategy}
        >
          <div className="space-y-3">
            {groups.map((group) => (
              <ReplacementGroupCard
                key={group.id}
                group={group}
                copy={copy}
                canAdd={count < RULE_LIMIT}
                canCopy={
                  customGroupCount < GROUP_LIMIT && count + russianFormalRules.length <= RULE_LIMIT
                }
                onChange={(next) =>
                  update(groups.map((item) => (item.id === group.id ? next : item)))
                }
                onRemove={() => update(groups.filter((item) => item.id !== group.id))}
                onCopy={() => {
                  const clone: ReplacementGroup = {
                    id: crypto.randomUUID(),
                    name: `${copy.textReplacementRussianPreset} (${copy.replacementCopySuffix})`,
                    enabled: true,
                    presetId: null,
                    rules: russianFormalRules.map((rule) => ({ ...rule })),
                  };
                  const next = groups.flatMap((item) =>
                    item.id === group.id ? [{ ...item, enabled: false }, clone] : [item],
                  );
                  onChange({ textReplacements: { enabled: true, groups: next } });
                }}
              />
            ))}
          </div>
        </SortableContext>
      </DndContext>
      <Button
        className="mt-4"
        variant="outline"
        size="sm"
        onClick={addGroup}
        disabled={customGroupCount >= GROUP_LIMIT}
      >
        <Plus />
        {copy.replacementAddGroup}
      </Button>
    </div>
  );
}

function ReplacementGroupCard({
  group,
  copy,
  canAdd,
  canCopy,
  onChange,
  onRemove,
  onCopy,
}: {
  group: ReplacementGroup;
  copy: Messages;
  canAdd: boolean;
  canCopy: boolean;
  onChange: (group: ReplacementGroup) => void;
  onRemove: () => void;
  onCopy: () => void;
}) {
  const system = group.presetId !== null;
  const [open, setOpen] = useState(!system);
  const displayName = system
    ? copy.textReplacementRussianPreset
    : group.name === "Custom" && group.id.startsWith("legacy-")
      ? copy.replacementCustomName
      : group.name;
  const [name, setName] = useState(displayName);
  useEffect(() => setName(displayName), [displayName]);
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: group.id,
  });
  return (
    <section
      ref={setNodeRef}
      aria-label={displayName}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      className={`overflow-hidden rounded-xl border border-border bg-card ${isDragging ? "relative z-10 shadow-lg" : ""}`}
    >
      <div className="flex flex-wrap items-center gap-2 p-3">
        <Button
          variant="ghost"
          size="icon-sm"
          className="touch-none cursor-grab"
          aria-label={copy.replacementDragGroup.replace("{name}", displayName)}
          {...attributes}
          {...listeners}
        >
          <GripVertical />
        </Button>
        <Switch
          aria-label={copy.replacementToggleGroup.replace("{name}", displayName)}
          checked={group.enabled}
          onCheckedChange={(enabled) => onChange({ ...group, enabled })}
        />
        {system ? (
          <span className="min-w-0 flex-1 text-sm font-medium">{displayName}</span>
        ) : (
          <Input
            className="min-w-28 flex-1"
            aria-label={copy.replacementGroupName}
            maxLength={80}
            value={name}
            onChange={(event) => setName(event.target.value)}
            onBlur={() => {
              const next = name.trim();
              if (next && next !== displayName) onChange({ ...group, name: next });
              else setName(displayName);
            }}
          />
        )}
        <span className="text-xs text-muted-foreground">
          {system ? copy.replacementSystem : copy.replacementCustom}
        </span>
        <span className="text-xs tabular-nums text-muted-foreground">
          {system ? russianFormalRules.length : (group.rules?.length ?? 0)}
        </span>
        {system ? (
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={copy.replacementCopy}
            title={copy.replacementCopy}
            disabled={!canCopy}
            onClick={onCopy}
          >
            <Copy />
          </Button>
        ) : (
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button variant="ghost" size="icon-sm" aria-label={copy.replacementDeleteGroup}>
                <Trash2 />
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>
                  {copy.replacementDeleteConfirm.replace("{name}", displayName)}
                </AlertDialogTitle>
                <AlertDialogDescription>{copy.replacementDeleteHint}</AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>{copy.cancel}</AlertDialogCancel>
                <AlertDialogAction variant="destructive" onClick={onRemove}>
                  {copy.delete}
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        )}
        <Button
          variant="ghost"
          size="icon-sm"
          aria-expanded={open}
          aria-label={open ? copy.replacementHideRules : copy.replacementShowRules}
          onClick={() => setOpen(!open)}
        >
          {open ? <ChevronUp /> : <ChevronDown />}
        </Button>
      </div>
      {open ? (
        <div className="border-t border-border p-3">
          {system ? (
            <>
              <p className="mb-3 text-xs text-muted-foreground">{copy.replacementSystemHint}</p>
              <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
                {russianFormalRules.map((rule) => (
                  <div key={rule.from} className="contents">
                    <dt>{rule.from}</dt>
                    <dd>{rule.to}</dd>
                  </div>
                ))}
              </dl>
            </>
          ) : (
            <CustomRules
              rules={group.rules ?? []}
              copy={copy}
              canAdd={canAdd}
              onChange={(rules) => onChange({ ...group, rules })}
            />
          )}
        </div>
      ) : null}
    </section>
  );
}

function CustomRules({
  rules,
  copy,
  canAdd,
  onChange,
}: {
  rules: TextReplacementRule[];
  copy: Messages;
  canAdd: boolean;
  onChange: (rules: TextReplacementRule[]) => void;
}) {
  const [ids, setIds] = useState<string[]>(() => rules.map(() => crypto.randomUUID()));
  if (ids.length !== rules.length)
    setIds(rules.map((_, index) => ids[index] ?? crypto.randomUUID()));
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );
  function add() {
    if (canAdd) onChange([...rules, { from: "", to: "", caseSensitive: false }]);
  }
  const addButton = (
    <Button type="button" variant="outline" size="sm" disabled={!canAdd} onClick={add}>
      <Plus />
      {copy.textReplacementAddRule}
    </Button>
  );
  return (
    <>
      <div className="mb-3">{addButton}</div>
      {rules.length ? (
        <>
          <div
            aria-hidden="true"
            className="grid grid-cols-[1.75rem_minmax(0,1fr)_minmax(0,1fr)_1.75rem_1.75rem] gap-2 px-1 pb-2 text-xs text-muted-foreground"
          >
            <span />
            <span>{copy.textReplacementFindLabel}</span>
            <span>{copy.textReplacementReplaceLabel}</span>
          </div>
          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            onDragEnd={({ active, over }) => {
              if (!over || active.id === over.id) return;
              const from = ids.indexOf(String(active.id)),
                to = ids.indexOf(String(over.id));
              if (from < 0 || to < 0) return;
              setIds(arrayMove(ids, from, to));
              onChange(arrayMove(rules, from, to));
            }}
            accessibility={{
              screenReaderInstructions: { draggable: copy.textReplacementDragInstructions },
              announcements: {
                onDragStart: ({ active }) =>
                  copy.textReplacementDragPicked.replace(
                    "{index}",
                    String(ids.indexOf(String(active.id)) + 1),
                  ),
                onDragOver: ({ over }) =>
                  over
                    ? copy.textReplacementDragPosition.replace(
                        "{index}",
                        String(ids.indexOf(String(over.id)) + 1),
                      )
                    : undefined,
                onDragEnd: ({ over }) =>
                  over ? copy.textReplacementDragDone : copy.textReplacementDragCancelled,
                onDragCancel: () => copy.textReplacementDragCancelled,
              },
            }}
          >
            <SortableContext items={ids} strategy={verticalListSortingStrategy}>
              <ol className="space-y-1">
                {rules.map((rule, index) => (
                  <ReplacementRow
                    key={ids[index]}
                    id={ids[index]}
                    rule={rule}
                    index={index}
                    copy={copy}
                    onChange={(patch) =>
                      onChange(rules.map((item, i) => (i === index ? { ...item, ...patch } : item)))
                    }
                    onRemove={() => {
                      setIds(ids.filter((_, i) => i !== index));
                      onChange(rules.filter((_, i) => i !== index));
                    }}
                  />
                ))}
              </ol>
            </SortableContext>
          </DndContext>
          <div className="mt-3">{addButton}</div>
        </>
      ) : (
        <p className="py-3 text-sm text-muted-foreground">{copy.textReplacementEmpty}</p>
      )}
    </>
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
            className={rule.caseSensitive ? "text-primary-text" : "text-muted-foreground"}
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
