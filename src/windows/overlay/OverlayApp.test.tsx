import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, describe, expect, it, vi } from "vitest";
import { OverlayApp } from "./OverlayApp";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "get_settings") {
      return { theme: "light", uiLanguage: "en", notifications: true };
    }
    if (cmd === "get_overlay_snapshot") {
      return { revision: 1, visible: true, state: { kind: "recording" } };
    }
    if (cmd === "get_session_state") {
      return { kind: "recording" };
    }
    if (cmd === "get_meter") {
      return { rms: 0.1, peak: 0.2, levels: Array.from({ length: 28 }, () => 0.2) };
    }
    return undefined;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));

afterEach(() => cleanup());

describe("OverlayApp", () => {
  it("names cancel as cancel and uses theme tokens", async () => {
    render(<OverlayApp />);
    const pill = await screen.findByRole("button", { name: /Hover to cancel|Наведите/ });
    expect(pill).toHaveClass("overlay-pill");
    fireEvent.click(pill);
    expect(invoke).not.toHaveBeenCalledWith("cancel_dictation");
    fireEvent.mouseEnter(pill);
    expect(pill).toHaveClass("overlay-pill-cancel");
    expect(pill).toHaveAccessibleName(/Cancel recording|Отменить запись/);
    expect(screen.getByText(/Cancel recording|Отменить запись/)).toHaveClass("overlay-center");
    expect(pill.querySelector(".overlay-wave")).toBeNull();
    expect(pill.querySelector(".overlay-dot")).toBeNull();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(invoke).not.toHaveBeenCalledWith("cancel_dictation");
    fireEvent.click(pill);
    expect(invoke).toHaveBeenCalledWith("cancel_dictation");
    fireEvent.mouseLeave(pill);
    expect(document.documentElement.lang).toBe("en");
  });
});
