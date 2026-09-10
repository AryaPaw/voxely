import { useEffect, useState } from "react";
import { api, type AppSettings } from "../../../lib/api";
import { meterFromPeakDb, peakDbFs, previewWarning } from "../../../lib/dsp-level";
import type { Messages } from "../../../lib/i18n";
import { PageHeader } from "../../../components/settings/PageHeader";
import { SettingsField } from "../../../components/settings/SettingsField";
import { Label } from "../../../components/ui/label";
import { Progress } from "../../../components/ui/progress";
import { SimpleSelect } from "../../../components/ui/simple-select";
import { SECTION_ICONS } from "../sectionNav";

export function AudioSettings({
  settings,
  copy,
  onChange,
}: {
  settings: AppSettings;
  copy: Messages;
  onChange: (patch: Partial<AppSettings>) => void;
}) {
  const [devices, setDevices] = useState<Array<{ id: string; name: string }>>([]);
  const [error, setError] = useState("");
  const [level, setLevel] = useState(0);
  const [warning, setWarning] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    void api
      .mics()
      .then((list) => {
        if (!cancelled) {
          setDevices(list);
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : "devices");
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);
  useEffect(() => {
    const timer = window.setInterval(() => {
      void api.meter().then((sample) => {
        setLevel(meterFromPeakDb(peakDbFs(sample.peak)));
        setWarning(previewWarning(sample.peak, 0, copy.tooQuiet, copy.clippingWarning));
      });
    }, 120);
    return () => window.clearInterval(timer);
  }, [copy.clippingWarning, copy.tooQuiet]);
  return (
    <div>
      <PageHeader icon={SECTION_ICONS.audio} title={copy.navAudio} />
      <SettingsField label={copy.inputDevice}>
        <SimpleSelect
          aria-label={copy.inputDevice}
          value={settings.inputDevice}
          onValueChange={(inputDevice) => onChange({ inputDevice })}
          options={[
            { value: "default", label: copy.defaultMic },
            ...devices.map((device) => ({ value: device.id, label: device.name })),
          ]}
        />
      </SettingsField>
      {error ? <p className="text-sm text-danger">{error}</p> : null}
      <div className="mt-4 max-w-lg">
        <Label className="mb-1 text-sm">{copy.micLevel}</Label>
        <Progress value={level} className="h-2" />
        {warning ? <p className="mt-1 text-xs text-danger">{warning}</p> : null}
      </div>
    </div>
  );
}
