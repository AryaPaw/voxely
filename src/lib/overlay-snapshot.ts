export function acceptOverlayRevision(current: number, incoming: number): boolean {
  return incoming > current;
}
