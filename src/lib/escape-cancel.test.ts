import { describe, expect, it, vi } from "vitest";
import { bindEscapeCancel, isEscapeCancelKey } from "./escape-cancel";

function key(partial: Partial<KeyboardEvent>): KeyboardEvent {
  return {
    key: "Escape",
    repeat: false,
    defaultPrevented: false,
    preventDefault() {
      Object.assign(this, { defaultPrevented: true });
    },
    ...partial,
  } as KeyboardEvent;
}

describe("escape-cancel", () => {
  it("accepts a fresh Escape", () => {
    expect(isEscapeCancelKey(key({}))).toBe(true);
    expect(isEscapeCancelKey(key({ key: "Enter" }))).toBe(false);
    expect(isEscapeCancelKey(key({ repeat: true }))).toBe(false);
  });

  it("listens on the capture phase", () => {
    const onCancel = vi.fn();
    const stop = bindEscapeCancel(onCancel);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    stop();
  });
});
