export function nextWriteSeq(current: number): number {
  return current + 1;
}

export function acceptSavedSettings<T extends { writeSeq?: number }>(prev: T | null, saved: T): T {
  return (saved.writeSeq ?? 0) >= (prev?.writeSeq ?? 0) ? saved : (prev ?? saved);
}

export function shouldKeepOptimistic<T extends { writeSeq?: number }>(
  prev: T | null,
  failedSeq: number,
): boolean {
  return Boolean(prev && (prev.writeSeq ?? 0) > failedSeq);
}
