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

export function reconcileFailedSettings<T extends { writeSeq?: number }>(
  prev: T | null,
  failedSeq: number,
  persisted: T,
): T {
  // A read-back after failure is authoritative for that draft even when its
  // stored sequence is lower. A later local write still owns its own result.
  return shouldKeepOptimistic(prev, failedSeq) ? acceptSavedSettings(prev, persisted) : persisted;
}
