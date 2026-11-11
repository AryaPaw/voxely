import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { api, type AppSettings } from "../../../lib/api";
import { meterFromPeakDb, peakDbFs, previewWarning } from "../../../lib/dsp-level";
import { formatInvokeError, type Messages } from "../../../lib/i18n";
import { PageHeader } from "../../../components/settings/PageHeader";
import { HotkeyCapture } from "../../../components/settings/HotkeyCapture";
import { AudioFilterSettings } from "./FilterSettings";
import { SettingsGroup } from "../../../components/settings/SettingsGroup";
import { SettingsField } from "../../../components/settings/SettingsField";
import { Label } from "../../../components/ui/label";
import { Progress } from "../../../components/ui/progress";
import { SimpleSelect } from "../../../components/ui/simple-select";
import { SECTION_ICONS } from "../sectionNav";

const METER_ERROR_RETRY_INITIAL_MS = 500;
const METER_ERROR_RETRY_MAX_MS = 8_000;

export function MicrophoneSettings({
  settings,
  copy,
  onChange,
}: {
  settings: AppSettings;
  copy: Messages;
  onChange: (patch: Partial<AppSettings>) => void;
}) {
  const [devices, setDevices] = useState<Array<{ id: string; name: string; available?: boolean }>>(
    [],
  );
  const [deviceError, setDeviceError] = useState("");
  const [meterError, setMeterError] = useState("");
  const [level, setLevel] = useState(0);
  const [warning, setWarning] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    void api
      .mics()
      .then((list) => {
        if (!cancelled) {
          setDevices(list);
          setDeviceError("");
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setDeviceError(formatInvokeError(err, copy));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [copy]);
  useEffect(() => {
    let cancelled = false;
    let starting = false;
    let started = false;
    let sampleRecording = false;
    let revision = 0;
    let retryAfter = 0;
    let nextErrorRetryMs = METER_ERROR_RETRY_INITIAL_MS;
    const owner = crypto.randomUUID();
    async function startMeter() {
      if (cancelled || starting || started || sampleRecording || Date.now() < retryAfter) return;
      starting = true;
      const requestRevision = revision;
      try {
        const acquired = await api.startInputMeter(owner);
        if (cancelled || requestRevision !== revision) {
          started = false;
          if (acquired) await api.stopInputMeter(owner);
          return;
        }
        started = acquired;
        if (started) {
          setMeterError("");
          retryAfter = 0;
          nextErrorRetryMs = METER_ERROR_RETRY_INITIAL_MS;
        }
        if (cancelled) await api.stopInputMeter(owner);
      } catch (err: unknown) {
        if (!cancelled) {
          setMeterError(formatInvokeError(err, copy));
          retryAfter = Date.now() + nextErrorRetryMs;
          nextErrorRetryMs = Math.min(nextErrorRetryMs * 2, METER_ERROR_RETRY_MAX_MS);
        }
      } finally {
        starting = false;
      }
    }
    void startMeter();
    const unlisten = listen("session://state", () => {
      revision++;
      started = false;
      retryAfter = 0;
      void startMeter();
    });
    const unlistenSample = listen<boolean>("filter://sample", ({ payload }) => {
      revision++;
      sampleRecording = payload;
      started = false;
      retryAfter = 0;
      if (!payload) void startMeter();
    });
    const timer = window.setInterval(() => {
      void startMeter();
      void api.meter().then((sample) => {
        if (cancelled) return;
        setLevel(meterFromPeakDb(peakDbFs(sample.peak)));
        setWarning(previewWarning(sample.peak, 0, copy.clippingWarning));
      });
    }, 120);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
      void unlisten.then((fn) => fn());
      void unlistenSample.then((fn) => fn());
      void api.stopInputMeter(owner);
    };
  }, [copy, settings.inputDevice]);
  return (
    <div>
      <PageHeader icon={SECTION_ICONS.audio} title={copy.navAudio} />
      <SettingsGroup title={copy.inputDevice}>
        <SettingsField label={copy.globalHotkey}>
          <HotkeyCapture
            value={settings.hotkey}
            prompt={copy.hotkeyPrompt}
            onChange={(hotkey) => onChange({ hotkey })}
          />
        </SettingsField>
        <SettingsField label={copy.inputDevice}>
          <SimpleSelect
            aria-label={copy.inputDevice}
            value={settings.inputDevice}
            onValueChange={(inputDevice) => onChange({ inputDevice })}
            options={[
              { value: "default", label: copy.defaultMic },
              ...devices.map((device) => ({
                value: device.id,
                label:
                  device.available === false
                    ? `${device.name} (${copy.deviceUnavailable})`
                    : device.name,
              })),
            ]}
          />
        </SettingsField>
        {deviceError ? (
          <p className="text-sm text-destructive" role="alert">
            {deviceError}
          </p>
        ) : null}
        {meterError ? (
          <p className="text-sm text-destructive" role="alert">
            {meterError}
          </p>
        ) : null}
        <div className="mt-4 max-w-lg">
          <Label className="mb-1 text-sm">{copy.micLevel}</Label>
          <Progress value={level} className="h-2" />
          {warning ? <p className="mt-1 text-xs text-destructive">{warning}</p> : null}
        </div>
      </SettingsGroup>
    </div>
  );
}

export function AudioSettings(props: {
  settings: AppSettings;
  copy: Messages;
  onChange: (patch: Partial<AppSettings>) => void;
  onOpenHistory?: () => void;
}) {
  return (
    <div>
      <MicrophoneSettings {...props} />
      <AudioFilterSettings {...props} />
    </div>
  );
}
