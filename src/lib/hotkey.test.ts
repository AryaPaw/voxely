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
  it.each([
    ["KeyQ", "я", "Q"],
    ["Digit7", "&", "7"],
    ["F12", "F12", "F12"],
    ["Enter", "Enter", "Enter"],
    ["Tab", "Tab", "Tab"],
  ])("uses physical %s independently of keyboard layout", (code, key, token) => {
    expect(hotkeyFromKeyboardEvent(event({ code, key, altKey: true, metaKey: true }))).toBe(
      `Alt+Super+${token}`,
    );
  });
  it.each(["F13", "Numpad1", "ArrowLeft", "KeyLong", "Digit12"])(
    "rejects unsupported code %s",
    (code) => {
      expect(hotkeyFromKeyboardEvent(event({ code, key: code, ctrlKey: true }))).toBeNull();
    },
  );
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
