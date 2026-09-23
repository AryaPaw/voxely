import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { OverlaySnapshot } from "../../lib/api";
import { OverlayApp } from "./OverlayApp";

const overlayHandlers: Array<(event: { payload: OverlaySnapshot }) => void> = [];

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
  listen: vi.fn(async (event: string, handler: (event: { payload: OverlaySnapshot }) => void) => {
    if (event === "overlay://snapshot") {
      overlayHandlers.push(handler);
    }
    return () => undefined;
  }),
}));

afterEach(() => {
  cleanup();
  overlayHandlers.length = 0;
});

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
    expect(pill).not.toHaveClass("overlay-pill-cancel");
    expect(document.documentElement.lang).toBe("en");
  });

  it("drops cancel copy after mouseleave even if :hover was latched", async () => {
    render(<OverlayApp />);
    const pill = await screen.findByRole("button", { name: /Hover to cancel|Наведите/ });
    fireEvent.mouseEnter(pill);
    expect(pill).toHaveClass("overlay-pill-cancel");
    fireEvent.mouseLeave(pill);
    expect(pill).not.toHaveClass("overlay-pill-cancel");
    expect(screen.queryByText(/Cancel recording|Отменить запись/)).not.toBeInTheDocument();
  });

  it("BUG-HUD-R1 keeps recording until a newer overlay revision arrives", async () => {
    render(<OverlayApp />);
    expect(await screen.findByText("Recording")).toBeInTheDocument();
    await waitFor(() => {
      expect(overlayHandlers.length).toBeGreaterThan(0);
    });
    overlayHandlers[0]({
      payload: { revision: 1, visible: true, state: { kind: "transcribing", attempt: 1 } },
    });
    expect(screen.getByText("Recording")).toBeInTheDocument();
    overlayHandlers[0]({
      payload: { revision: 2, visible: true, state: { kind: "transcribing", attempt: 1 } },
    });
    await waitFor(() => {
      expect(screen.getByText("Transcribing")).toBeInTheDocument();
    });
  });

  it("hides leftover HUD when backend stays visible after idle", async () => {
    render(<OverlayApp />);
    expect(await screen.findByText("Recording")).toBeInTheDocument();
    await waitFor(() => {
      expect(overlayHandlers.length).toBeGreaterThan(0);
    });
    overlayHandlers[0]({
      payload: { revision: 4, visible: true, state: { kind: "idle" } },
    });
    await waitFor(() => {
      expect(screen.queryByRole("button")).not.toBeInTheDocument();
      expect(screen.queryByText("Transcribing")).not.toBeInTheDocument();
      expect(screen.queryByText("Recording")).not.toBeInTheDocument();
    });
  });

  it("shows the recording limit HUD when the capture cap is hit", async () => {
    render(<OverlayApp />);
    expect(await screen.findByText("Recording")).toBeInTheDocument();
    await waitFor(() => {
      expect(overlayHandlers.length).toBeGreaterThan(0);
    });
    overlayHandlers[0]({
      payload: {
        revision: 3,
        visible: true,
        state: { kind: "recording" },
        limitReached: true,
      },
    });
    expect(await screen.findByText("Recording limit")).toBeInTheDocument();
  });
});
