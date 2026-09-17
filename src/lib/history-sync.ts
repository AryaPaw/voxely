export const HISTORY_CHANGED = "history://changed";

export function shouldReloadHistory(event: string): boolean {
  return event === HISTORY_CHANGED;
}

export function historySearchQuery(query: string): string | undefined {
  const trimmed = query.trim();
  return trimmed ? trimmed : undefined;
}
