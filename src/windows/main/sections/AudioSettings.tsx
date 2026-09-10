import { useEffect, useState } from "react";
import { api, type AppSettings } from "../../../lib/api";
import type { Messages } from "../../../lib/i18n";
import { PageHeader } from "../../../components/settings/PageHeader";
import { SettingsField } from "../../../components/settings/SettingsField";
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
    </div>
  );
}
