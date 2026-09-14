export const HISTORY_CHANGED = "history://changed";

export function shouldReloadHistory(event: string): boolean {
  return event === HISTORY_CHANGED;
}
