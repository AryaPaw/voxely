const MOD_KEYS = new Set(["Control", "Shift", "Alt", "Meta", "OS"]);

export function hotkeyFromKeyboardEvent(event: KeyboardEvent): string | null {
  if (MOD_KEYS.has(event.key)) {
    return null;
  }
  const token = keyToken(event);
  if (!token || token === "Escape") {
    return null;
  }
  const parts: string[] = [];
  if (event.ctrlKey) {
    parts.push("Ctrl");
  }
  if (event.altKey) {
    parts.push("Alt");
  }
  if (event.metaKey) {
    parts.push("Super");
  }
  if (event.shiftKey) {
    parts.push("Shift");
  }
  parts.push(token);
  if (parts.length < 2) {
    return null;
  }
  return parts.join("+");
}

function keyToken(event: KeyboardEvent): string | null {
  if (event.code === "Space") {
    return "Space";
  }
  if (event.code.startsWith("Key") && event.code.length === 4) {
    return event.code.slice(3);
  }
  if (event.code.startsWith("Digit") && event.code.length === 6) {
    return event.code.slice(5);
  }
  if (/^F([1-9]|1[0-2])$/.test(event.code)) {
    return event.code;
  }
  if (event.code === "Enter") {
    return "Enter";
  }
  if (event.code === "Tab") {
    return "Tab";
  }
  return null;
}
