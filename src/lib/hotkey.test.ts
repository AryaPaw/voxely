import { describe, expect, it } from "vitest";
import { hotkeyFromKeyboardEvent } from "./hotkey";

function event(partial: Partial<KeyboardEvent>): KeyboardEvent {
  return {
    key: " ",
    code: "Space",
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    metaKey: false,
    ...partial,
  } as KeyboardEvent;
}

describe("hotkeyFromKeyboardEvent", () => {
  it("ignores modifier-only presses", () => {
    expect(hotkeyFromKeyboardEvent(event({ key: "Control", code: "ControlLeft" }))).toBeNull();
  });

  it("requires a modifier", () => {
    expect(hotkeyFromKeyboardEvent(event({ key: "a", code: "KeyA" }))).toBeNull();
  });

  it("captures ctrl+shift+space", () => {
    expect(
      hotkeyFromKeyboardEvent(event({ key: " ", code: "Space", ctrlKey: true, shiftKey: true })),
    ).toBe("Ctrl+Shift+Space");
  });

  it("does not bind Escape", () => {
    expect(
      hotkeyFromKeyboardEvent(event({ key: "Escape", code: "Escape", ctrlKey: true })),
    ).toBeNull();
  });
});
