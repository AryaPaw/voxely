import { memo, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  api,
  type AppSettings,
  type DspPreset,
  type DspPreview,
  type FilterKind,
} from "../../../lib/api";
import { meterFromPeakDb, peakDbFs, previewWarning } from "../../../lib/dsp-level";
import { formatInvokeError, factoryPresetLabel, type Messages } from "../../../lib/i18n";
import { PageHeader } from "../../../components/settings/PageHeader";
import { SettingsSwitchRow } from "../../../components/settings/SettingsSwitchRow";
import { Button } from "../../../components/ui/button";
import { Label } from "../../../components/ui/label";
import { Progress } from "../../../components/ui/progress";
import { SimpleSelect } from "../../../components/ui/simple-select";
import { Slider } from "../../../components/ui/slider";
import { SECTION_ICONS, sectionLabel } from "../sectionNav";

const DSP_PREVIEW_DEBOUNCE_MS = 400;

export function FilterSettings({
  settings,
  copy,
  onChange,
}: {
  settings: AppSettings;
  copy: Messages;
  onChange: (patch: Partial<AppSettings>) => void;
}) {
  const [unsupported, setUnsupported] = useState<string[]>([]);
  const [preview, setPreview] = useState<DspPreview | null>(null);
  const [previewError, setPreviewError] = useState("");
  const [recording, setRecording] = useState(false);
  const [busy, setBusy] = useState(false);
  const preset = activePreset(settings);
  const signature = presetSignature(preset);

  useEffect(() => {
    if (recording) {
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void api
        .previewDsp()
        .then((next) => {
          if (!cancelled) {
            setPreview(next);
            setPreviewError("");
          }
        })
        .catch((error: unknown) => {
          if (!cancelled) {
            setPreview(null);
            setPreviewError(formatInvokeError(error, copy));
          }
        });
    }, DSP_PREVIEW_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
    // copy is display-only for the error string
  }, [copy, recording, settings.activePresetId, signature]);

  async function toggleSample() {
    setPreviewError("");
    if (recording) {
      setBusy(true);
      try {
        setPreview(await api.stopFilterSample());
        setRecording(false);
      } catch (error) {
        setPreviewError(formatInvokeError(error, copy));
      } finally {
        setBusy(false);
      }
      return;
    }
    setBusy(true);
    try {
      await api.startFilterSample();
      setRecording(true);
      setPreview(null);
    } catch (error) {
      setPreviewError(formatInvokeError(error, copy));
    } finally {
      setBusy(false);
    }
  }

  function patchPreset(next: DspPreset) {
    onChange({
      presets: settings.presets.map((item) => (item.id === next.id ? next : item)),
    });
  }

  return (
    <div>
      <PageHeader icon={SECTION_ICONS.filters} title={sectionLabel(copy, "filters")} />
      <p className="mb-3 max-w-lg text-sm text-muted-foreground">{copy.filtersIntro}</p>
      <ol className="mb-4 max-w-lg list-decimal space-y-1 pl-5 text-sm text-muted-foreground">
        <li>{copy.recordSample}</li>
        <li>
          {copy.playOriginal} / {copy.playProcessed}
        </li>
      </ol>
      <label className="mb-4 block max-w-lg">
        <div className="mb-1 text-sm">{copy.activePreset}</div>
        <SimpleSelect
          aria-label={copy.activePreset}
          value={settings.activePresetId}
          onValueChange={(activePresetId) => onChange({ activePresetId })}
          options={settings.presets.map((item) => ({
            value: item.id,
            label: factoryPresetLabel(item, copy),
          }))}
        />
      </label>
      <MicLevelMeter live={recording} copy={copy} />
      {preset ? (
        <>
          <TuneSlider
            label={copy.gainDb.replace("{value}", preset.gain.db.toFixed(1))}
            min={-12}
            max={18}
            step={0.5}
            value={preset.gain.db}
            onChange={(db) => patchPreset({ ...preset, gain: { ...preset.gain, db } })}
          />
          <TuneSlider
            label={copy.highpass.replace("{value}", String(Math.round(preset.highPass.cutoffHz)))}
            min={20}
            max={200}
            step={5}
            value={preset.highPass.cutoffHz}
            onChange={(cutoffHz) =>
              patchPreset({
                ...preset,
                highPass: { ...preset.highPass, cutoffHz },
                order: setSlot(preset.order, "highPass", true),
              })
            }
          />
          <SettingsSwitchRow
            label={copy.filterRnnoise}
            checked={slotEnabled(preset, "rnnoise")}
            onCheckedChange={(enabled) =>
              patchPreset({ ...preset, order: setSlot(preset.order, "rnnoise", enabled) })
            }
          />
          <SettingsSwitchRow
            label={copy.filterCompressor}
            checked={slotEnabled(preset, "compressor")}
            onCheckedChange={(enabled) =>
              patchPreset({ ...preset, order: setSlot(preset.order, "compressor", enabled) })
            }
          />
          <SettingsSwitchRow
            label={copy.filterExpander}
            checked={slotEnabled(preset, "expander")}
            onCheckedChange={(enabled) =>
              patchPreset({ ...preset, order: setSlot(preset.order, "expander", enabled) })
            }
          />
          <SettingsSwitchRow
            label={copy.filterGate}
            checked={slotEnabled(preset, "gate")}
            onCheckedChange={(enabled) =>
              patchPreset({ ...preset, order: setSlot(preset.order, "gate", enabled) })
            }
          />
          <SettingsSwitchRow
            label={copy.filterLimiter}
            checked={slotEnabled(preset, "limiter")}
            onCheckedChange={(enabled) =>
              patchPreset({ ...preset, order: setSlot(preset.order, "limiter", enabled) })
            }
          />
        </>
      ) : null}
      <div className="mb-4 flex flex-wrap gap-2">
        <Button disabled={busy} onClick={() => void toggleSample()}>
          {recording ? copy.stopSample : copy.recordSample}
        </Button>
        <Button
          variant="outline"
          onClick={async () => {
            const previewObs = await api.obsPreview();
            const first = previewObs[0];
            if (!first) {
              setUnsupported([copy.obsMicMissing]);
              return;
            }
            setUnsupported(first.unsupported);
            await api.importObs(first.sourceName, `OBS ${first.sourceName}`);
          }}
        >
          {copy.importObs}
        </Button>
      </div>
      {recording ? (
        <p className="mb-3 text-sm text-muted-foreground">{copy.recordingSample}</p>
      ) : null}
      {previewError ? (
        <p className="mb-3 text-sm text-danger">
          {previewError.includes("no filter sample") ? copy.noFilterSample : previewError}
        </p>
      ) : null}
      {preview ? <FilterPreviewPlayer preview={preview} copy={copy} /> : null}
      {unsupported.length > 0 ? (
        <ul className="mt-3 text-sm text-muted-foreground">
          {unsupported.map((item) => (
            <li key={item}>{copy.unsupported.replace("{item}", item)}</li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

function activePreset(settings: AppSettings): DspPreset | undefined {
  return (
    settings.presets.find((preset) => preset.id === settings.activePresetId) ?? settings.presets[0]
  );
}

function presetSignature(preset: DspPreset | undefined): string {
  return JSON.stringify(preset ?? null);
}

function slotEnabled(preset: DspPreset, kind: FilterKind): boolean {
  return preset.order.some((slot) => slot.kind === kind && slot.enabled);
}

function setSlot(
  order: DspPreset["order"],
  kind: FilterKind,
  enabled: boolean,
): DspPreset["order"] {
  if (order.some((slot) => slot.kind === kind)) {
    return order.map((slot) => (slot.kind === kind ? { ...slot, enabled } : slot));
  }
  return [...order, { id: kind, kind, enabled }];
}

function MicLevelMeter({ live, copy }: { live: boolean; copy: Messages }) {
  const [level, setLevel] = useState(0);
  const [warning, setWarning] = useState<string | null>(null);
  useEffect(() => {
    if (!live) {
      setLevel(0);
      setWarning(null);
      return;
    }
    const timer = window.setInterval(() => {
      void api.meter().then((sample) => {
        const db = peakDbFs(sample.peak);
        setLevel(meterFromPeakDb(db));
        setWarning(previewWarning(sample.peak, 0, copy.clippingWarning));
      });
    }, 80);
    return () => window.clearInterval(timer);
  }, [copy.clippingWarning, live]);
  return (
    <div className="mb-5 max-w-lg">
      <Label className="mb-1 text-sm">{copy.micLevel}</Label>
      <Progress value={level} className="h-2" />
      <p className="mt-1 text-xs text-muted-foreground">{live ? copy.meterLive : copy.meterIdle}</p>
      {warning ? <p className="mt-1 text-xs text-danger">{warning}</p> : null}
    </div>
  );
}

const FilterPreviewPlayer = memo(function FilterPreviewPlayer({
  preview,
  copy,
}: {
  preview: DspPreview;
  copy: Messages;
}) {
  const originalRef = useRef<HTMLAudioElement>(null);
  const processedRef = useRef<HTMLAudioElement>(null);
  const originalSrc =
    preview.originalDataUrl || convertFallback(preview.originalPath, preview.nonce);
  const processedSrc =
    preview.processedDataUrl || convertFallback(preview.processedPath, preview.nonce);
  const peak = peakDbFs(preview.peak);
  const warning = previewWarning(preview.peak, preview.clipCount, copy.clippingWarning);

  function playSide(side: "original" | "processed") {
    const original = originalRef.current;
    const processed = processedRef.current;
    const target = side === "original" ? original : processed;
    const other = side === "original" ? processed : original;
    if (!target) {
      return;
    }
    const playing = [original, processed].find((node) => node && !node.paused);
    const time = playing?.currentTime ?? target.currentTime;
    other?.pause();
    target.currentTime = time;
    void target.play();
  }

  return (
    <div className="mb-4 max-w-lg rounded-xl border border-border bg-card p-3 text-sm">
      <p className="mb-2 text-muted-foreground">
        {copy.peakRms
          .replace("{peak}", Number.isFinite(peak) ? peak.toFixed(1) : "-inf")
          .replace("{rms}", preview.rms.toFixed(3))}
        {preview.clipCount > 0 ? copy.clipping.replace("{count}", String(preview.clipCount)) : ""}
      </p>
      {warning ? <p className="mb-2 text-xs text-danger">{warning}</p> : null}
      <div className="mb-3 flex items-center gap-2">
        <Button variant="outline" onClick={() => playSide("original")}>
          {copy.playOriginal}
        </Button>
        <Button onClick={() => playSide("processed")}>{copy.playProcessed}</Button>
      </div>
      <audio ref={originalRef} preload="auto" src={originalSrc} />
      <audio ref={processedRef} preload="auto" src={processedSrc} />
    </div>
  );
});

function convertFallback(path: string, nonce?: number): string {
  const src = convertFileSrc(path.replace(/\\/g, "/"));
  return typeof nonce === "number" ? `${src}?n=${nonce}` : src;
}

function TuneSlider({
  label,
  min,
  max,
  step,
  value,
  onChange,
}: {
  label: string;
  min: number;
  max: number;
  step: number;
  value: number;
  onChange: (value: number) => void;
}) {
  return (
    <div className="mb-4 max-w-lg">
      <Label className="mb-1 text-sm">{label}</Label>
      <Slider
        min={min}
        max={max}
        step={step}
        value={[value]}
        aria-label={label}
        onValueChange={(next) => {
          const first = next[0];
          if (typeof first === "number") {
            onChange(first);
          }
        }}
      />
    </div>
  );
}
