import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { api, type AppSettings, type RuntimeInfo } from "../../../lib/api";
import { appDisplayName, formatInvokeError, statusToast, type Messages } from "../../../lib/i18n";
import { formatReleaseDate } from "../../../lib/release-meta";
import { reportError } from "../../../lib/system-notify";
import { PageHeader } from "../../../components/settings/PageHeader";
import { SettingsGroup } from "../../../components/settings/SettingsGroup";
import { SettingsField } from "../../../components/settings/SettingsField";
import { SettingsSwitchRow } from "../../../components/settings/SettingsSwitchRow";
import { Button } from "../../../components/ui/button";
import { SECTION_ICONS } from "../sectionNav";

export function AdvancedSettings({
  settings,
  copy,
  onChange,
}: {
  settings: AppSettings;
  copy: Messages;
  onChange: (patch: Partial<AppSettings>) => void;
}) {
  const [info, setInfo] = useState<RuntimeInfo | null>(null);
  const [version, setVersion] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    void Promise.allSettled([api.runtimeInfo(), getVersion()]).then(([runtime, nextVersion]) => {
      if (!active) return;
      if (runtime.status === "fulfilled") setInfo(runtime.value);
      if (nextVersion.status === "fulfilled") setVersion(nextVersion.value);
      const failed =
        runtime.status === "rejected"
          ? runtime.reason
          : nextVersion.status === "rejected"
            ? nextVersion.reason
            : null;
      setError(failed ? formatInvokeError(failed, copy) : "");
      setLoading(false);
    });
    return () => {
      active = false;
    };
  }, [copy, revision]);
  function openDir(kind: "logs" | "settings") {
    const open = kind === "logs" ? api.openLogs : api.openSettingsDir;
    const failed = kind === "logs" ? copy.openLogsFailed : copy.openSettingsFailed;
    void open().catch(() => reportError(statusToast("error", copy, failed)));
  }
  return (
    <div>
      <PageHeader icon={SECTION_ICONS.advanced} title={copy.navAdvanced} />
      <SettingsGroup title={copy.diagnosticsLogging}>
        <SettingsSwitchRow
          label={copy.debugLogs}
          description={copy.diagnosticsLoggingHint}
          checked={settings.debugLogging}
          onCheckedChange={(debugLogging) => onChange({ debugLogging })}
        />
      </SettingsGroup>
      <SettingsGroup title={copy.diagnosticsFiles}>
        <SettingsField label={copy.diagnosticsLogs} description={copy.diagnosticsLogsHint} action>
          <Button variant="outline" onClick={() => openDir("logs")}>
            {copy.openLogs}
          </Button>
        </SettingsField>
        <SettingsField label={copy.diagnosticsData} description={copy.diagnosticsDataHint} action>
          <Button variant="outline" onClick={() => openDir("settings")}>
            {copy.openSettingsFolder}
          </Button>
        </SettingsField>
      </SettingsGroup>
      <SettingsGroup title={copy.diagnosticsBuild}>
        <dl className="settings-group-rows">
          <div className="settings-info-row">
            <dt>{copy.diagnosticsEdition}</dt>
            <dd>
              {info
                ? appDisplayName(copy, info.localBuild)
                : loading
                  ? copy.loading
                  : copy.diagnosticsUnavailable}
            </dd>
          </div>
          <div className="settings-info-row">
            <dt>{copy.aboutVersion}</dt>
            <dd>{version || (loading ? copy.loading : copy.diagnosticsUnavailable)}</dd>
          </div>
          <div className="settings-info-row">
            <dt>{copy.diagnosticsBuildDate}</dt>
            <dd>
              {info
                ? formatReleaseDate(info.buildDate, copy.dateLocale)
                : loading
                  ? copy.loading
                  : copy.diagnosticsUnavailable}
            </dd>
          </div>
        </dl>
        {error ? (
          <div className="space-y-3 py-3 text-sm" role="alert">
            <p className="text-destructive">{error}</p>
            <Button variant="outline" onClick={() => setRevision((value) => value + 1)}>
              {copy.retryAction}
            </Button>
          </div>
        ) : null}
      </SettingsGroup>
    </div>
  );
}
