export type OverlayRevisioned = {
  revision: number;
};

export function acceptOverlayRevision(current: number, incoming: number): boolean {
  return incoming > current;
}

export function applyOverlaySnapshot<T extends OverlayRevisioned>(current: T, incoming: T): T {
  return acceptOverlayRevision(current.revision, incoming.revision) ? incoming : current;
}
