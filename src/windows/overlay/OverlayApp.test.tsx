import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { OverlayApp } from "./OverlayApp";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "get_settings") {
      return { theme: "light", uiLanguage: "en", notifications: true };
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
    const pill = await screen.findByRole("button", { name: /Cancel recording|Отменить запись/ });
    expect(pill).toHaveClass("overlay-pill");
    fireEvent.mouseEnter(pill);
    expect(pill).toHaveClass("overlay-pill-armed");
    fireEvent.mouseLeave(pill);
    expect(document.documentElement.lang).toBe("en");
  });
});
