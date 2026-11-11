export function VersionBadge({ version }: { version: string }) {
  return <span className="version-badge">v{version || "…"}</span>;
}
