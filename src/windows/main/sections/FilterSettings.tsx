import { memo, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  api,
  defaultMicTune,
  type AppSettings,
  type DspPreview,
  type MicTune,
} from "../../../lib/api";
import { formatInvokeError, type Messages } from "../../../lib/i18n";
import { PageHeader } from "../../../components/settings/PageHeader";
import { Button } from "../../../components/ui/button";
import { Label } from "../../../components/ui/label";
import { Progress } from "../../../components/ui/progress";
import { SimpleSelect } from "../../../components/ui/simple-select";
import { Slider } from "../../../components/ui/slider";
import { SECTION_ICONS } from "../sectionNav";

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
  const [tune, setTune] = useState<MicTune>(settings.micTune ?? defaultMicTune);
  const [preview, setPreview] = useState<DspPreview | null>(null);
  const [previewError, setPreviewError] = useState("");
  const [recording, setRecording] = useState(false);
  const [busy, setBusy] = useState(false);

  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  useEffect(() => {
    setTune(settings.micTune ?? defaultMicTune);
  }, [settings.micTune]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      const current = settings.micTune ?? defaultMicTune;
      if (
        current.gainDb === tune.gainDb &&
        current.highpassHz === tune.highpassHz &&
        current.denoise === tune.denoise &&
        current.punch === tune.punch
      ) {
        return;
      }
      onChangeRef.current({ micTune: tune });
    }, 280);
    return () => window.clearTimeout(timer);
  }, [settings.micTune, tune]);

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

  return (
    <div>
      <PageHeader icon={SECTION_ICONS.filters} title={copy.filtersTitle} />
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
          options={settings.presets.map((preset) => ({ value: preset.id, label: preset.name }))}
        />
      </label>
      <MicLevelMeter live={recording} copy={copy} />
      <TuneSlider
        label={copy.gainDb.replace("{value}", tune.gainDb.toFixed(1))}
        min={-12}
        max={18}
        step={0.5}
        value={tune.gainDb}
        onChange={(gainDb) => setTune({ ...tune, gainDb })}
      />
      <TuneSlider
        label={copy.highpass.replace("{value}", String(Math.round(tune.highpassHz)))}
        min={20}
        max={200}
        step={5}
        value={tune.highpassHz}
        onChange={(highpassHz) => setTune({ ...tune, highpassHz })}
      />
      <TuneSlider
        label={copy.denoise.replace("{value}", String(tune.denoise))}
        min={0}
        max={100}
        step={1}
        value={tune.denoise}
        onChange={(denoise) => setTune({ ...tune, denoise })}
      />
      <TuneSlider
        label={copy.punch.replace("{value}", String(tune.punch))}
        min={0}
        max={100}
        step={1}
        value={tune.punch}
        onChange={(punch) => setTune({ ...tune, punch })}
      />
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
      {previewError ? <p className="mb-3 text-sm text-danger">{previewError}</p> : null}
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

function MicLevelMeter({ live, copy }: { live: boolean; copy: Messages }) {
  const [level, setLevel] = useState(0);
  useEffect(() => {
    if (!live) {
      setLevel(0);
      return;
    }
    const timer = window.setInterval(() => {
      void api.meter().then((sample) => {
        setLevel(Math.min(1, sample.rms * 12 + sample.peak * 0.4));
      });
    }, 80);
    return () => window.clearInterval(timer);
  }, [live]);
  return (
    <div className="mb-5 max-w-lg">
      <Label className="mb-1 text-sm">{copy.micLevel}</Label>
      <Progress value={level * 100} className="h-2" />
      <p className="mt-1 text-xs text-muted-foreground">{live ? copy.meterLive : copy.meterIdle}</p>
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
  const originalSrc = preview.originalDataUrl ?? convertFallback(preview.originalPath);
  const processedSrc = preview.processedDataUrl ?? convertFallback(preview.processedPath);

  return (
    <div className="mb-4 max-w-lg rounded-xl border border-border bg-surface p-3 text-sm">
      <p className="mb-2 text-muted-foreground">
        {copy.peakRms
          .replace("{peak}", (preview.peak * 100).toFixed(0))
          .replace("{rms}", (preview.rms * 100).toFixed(0))}
        {preview.clipCount > 0 ? copy.clipping.replace("{count}", String(preview.clipCount)) : ""}
      </p>
      <div className="mb-3 flex items-center gap-2">
        <Button
          variant="outline"
          onClick={() => {
            processedRef.current?.pause();
            void originalRef.current?.play();
          }}
        >
          {copy.playOriginal}
        </Button>
        <Button
          onClick={() => {
            originalRef.current?.pause();
            void processedRef.current?.play();
          }}
        >
          {copy.playProcessed}
        </Button>
      </div>
      <audio ref={originalRef} preload="auto" src={originalSrc} />
      <audio ref={processedRef} preload="auto" src={processedSrc} />
    </div>
  );
});

function convertFallback(path: string): string {
  return convertFileSrc(path.replace(/\\/g, "/"));
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
