export function isEscapeCancelKey(event: KeyboardEvent): boolean {
  return event.key === "Escape" && !event.repeat && !event.defaultPrevented;
}

export function bindEscapeCancel(onCancel: () => void): () => void {
  const onKey = (event: KeyboardEvent) => {
    if (!isEscapeCancelKey(event)) {
      return;
    }
    event.preventDefault();
    onCancel();
  };
  window.addEventListener("keydown", onKey, true);
  return () => window.removeEventListener("keydown", onKey, true);
}
