export const COMPARE_MODEL_MIN = 2;
export const COMPARE_MODEL_MAX = 12;

export function moveCompareModel(models: string[], from: number, to: number): string[] {
  if (from === to || from < 0 || to < 0 || from >= models.length || to >= models.length) {
    return models;
  }
  const next = [...models];
  const [item] = next.splice(from, 1);
  next.splice(to, 0, item);
  return next;
}

export function compareSlotIds(count: number): string[] {
  return Array.from({ length: count }, (_, index) => `compare-slot-${index}`);
}

export function formatCompareCost(value: number): string {
  return String(value);
}
