import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { GripVertical } from "lucide-react";
import type { AppSettings, CompareSlot } from "../../../lib/api";
import { formatInvokeError, messagesForUiLanguage, type Messages } from "../../../lib/i18n";
import { Button } from "../../../components/ui/button";
import { Input } from "../../../components/ui/input";
import { formatCompareCost } from "./compare-models";

export function CompareModelCard({
  id,
  index,
  model,
  settings,
  copy,
  slot,
  canRemove,
  onModelChange,
  onRemove,
  onMakeDefault,
}: {
  id: string;
  index: number;
  model: string;
  settings: AppSettings;
  copy: Messages;
  slot: CompareSlot | undefined;
  canRemove: boolean;
  onModelChange: (value: string) => void;
  onRemove: () => void;
  onMakeDefault: () => void;
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
  const trimmed = model.trim();
  const isDefault = trimmed.length > 0 && trimmed === settings.model;
  const statusCopy = messagesForUiLanguage(settings.uiLanguage ?? "auto", navigator.language);
  const status = slot
    ? ({
        running: statusCopy.compareRunning,
        done: statusCopy.compareDone,
        cancelled: statusCopy.compareCancelled,
        error: statusCopy.compareSlotError,
      }[slot.status] ?? slot.status)
    : "";

  return (
    <div
      ref={setNodeRef}
      style={{
        transform: CSS.Transform.toString(transform),
        transition,
      }}
      className={`rounded-lg border border-border p-3 ${isDragging ? "z-10 opacity-80" : ""}`}
    >
      <div className="mb-2 flex items-center gap-2">
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          ref={setActivatorNodeRef}
          aria-label={copy.compareDrag}
          {...attributes}
          {...listeners}
        >
          <GripVertical />
        </Button>
        <Input
          aria-label={`model-${index + 1}`}
          value={model}
          spellCheck={false}
          autoComplete="off"
          onChange={(event) => onModelChange(event.target.value)}
        />
      </div>
      <div className="flex gap-2">
        {canRemove ? (
          <Button type="button" variant="ghost" onClick={onRemove}>
            {copy.compareRemoveModel}
          </Button>
        ) : null}
        <Button
          type="button"
          variant={isDefault ? "default" : "outline"}
          disabled={!trimmed || isDefault}
          aria-pressed={isDefault}
          onClick={onMakeDefault}
        >
          {isDefault ? copy.compareIsDefault : copy.compareMakeDefault}
        </Button>
      </div>
      {slot ? (
        <div className="mt-3 text-sm">
          <div className="text-muted-foreground">
            {status} / {copy.compareAttempt.replace("{value}", String(slot.attempt))}
            {slot.cost != null
              ? ` / ${copy.compareCost.replace("{value}", formatCompareCost(slot.cost))} USD`
              : ` / ${statusCopy.compareUnknownCost}`}
            {slot.latencyMs != null
              ? ` / ${(slot.latencyMs / 1000).toFixed(2)} ${statusCopy.compareLatencyUnit}`
              : ""}
          </div>
          {slot.text ? <p className="mt-1 whitespace-pre-wrap">{slot.text}</p> : null}
          {slot.error ? (
            <p className="mt-1 text-destructive">{formatInvokeError({ code: slot.error }, copy)}</p>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
