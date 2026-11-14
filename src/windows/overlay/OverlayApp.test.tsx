import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { AppSettings, OverlaySnapshot } from "../../lib/api";
import { OverlayApp } from "./OverlayApp";

const overlayHandlers: Array<(event: { payload: OverlaySnapshot }) => void> = [];
const settingsHandlers: Array<(event: { payload: AppSettings }) => void> = [];
const pointerCheck = vi.hoisted(() => ({
  matches: true,
  pending: undefined as Promise<boolean> | undefined,
}));
const originalPointerEvent = window.PointerEvent;

beforeAll(() => {
  Object.defineProperty(window, "PointerEvent", {
    configurable: true,
    value: window.MouseEvent,
  });
});

afterAll(() => {
  if (originalPointerEvent) {
    Object.defineProperty(window, "PointerEvent", {
      configurable: true,
      value: originalPointerEvent,
    });
  } else {
    Reflect.deleteProperty(window, "PointerEvent");
  }
});

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "overlay_pointer_matches") {
      return pointerCheck.pending ?? pointerCheck.matches;
    }
    if (cmd === "get_settings") {
      return { theme: "light", uiLanguage: "en", notifications: true };
    }
    if (cmd === "get_overlay_snapshot") {
      return { revision: 1, generation: 1, epoch: 1, visible: true, state: { kind: "recording" } };
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
  listen: vi.fn(
    async (event: string, handler: (event: { payload: OverlaySnapshot | AppSettings }) => void) => {
      if (event === "overlay://snapshot") {
        overlayHandlers.push(handler);
      }
      if (event === "settings://changed") settingsHandlers.push(handler);
      return () => undefined;
    },
  ),
}));

afterEach(() => {
  cleanup();
  overlayHandlers.length = 0;
  settingsHandlers.length = 0;
  pointerCheck.matches = true;
  pointerCheck.pending = undefined;
  vi.mocked(invoke).mockClear();
});

describe("OverlayApp", () => {
  it("keeps the accepted explicit theme when an old system-media callback runs before commit", async () => {
    const original = vi.mocked(invoke).getMockImplementation()!;
    const media = new window.EventTarget() as EventTarget & { matches: boolean };
    media.matches = false;
    vi.stubGlobal("matchMedia", () => media);
    vi.mocked(invoke).mockImplementation((command, args, options) =>
      command === "get_settings"
        ? Promise.resolve({ theme: "system", writeSeq: 1, uiLanguage: "en" })
        : original(command, args, options),
    );
    try {
      render(<OverlayApp />);
      await waitFor(() => expect(settingsHandlers).toHaveLength(1));
      await waitFor(() => expect(document.documentElement.dataset.theme).toBe("light"));
      await act(async () => {
        // Both browser callbacks run before the pending React commit.
        await new Promise<void>((resolve) => {
          window.setTimeout(
            () =>
              settingsHandlers[0]({
                payload: { theme: "dark", writeSeq: 2, uiLanguage: "en" } as AppSettings,
              }),
            0,
          );
          window.setTimeout(() => {
            media.dispatchEvent(new window.Event("change"));
            resolve();
          }, 0);
        });
      });
      expect(document.documentElement.dataset.theme).toBe("dark");
    } finally {
      vi.mocked(invoke).mockImplementation(original);
      vi.unstubAllGlobals();
    }
  });

  it("does not reapply stale theme and locale from a late initial settings read", async () => {
    const original = vi.mocked(invoke).getMockImplementation()!;
    let resolveInitial!: (settings: AppSettings) => void;
    const pending = new Promise<AppSettings>((resolve) => {
      resolveInitial = resolve;
    });
    vi.mocked(invoke).mockImplementation((command, args, options) =>
      command === "get_settings" ? pending : original(command, args, options),
    );
    try {
      render(<OverlayApp />);
      await waitFor(() => expect(settingsHandlers).toHaveLength(1));
      await act(async () => {
        settingsHandlers[0]({
          payload: { theme: "dark", uiLanguage: "en", writeSeq: 2 } as AppSettings,
        });
      });
      expect(document.documentElement.dataset.theme).toBe("dark");
      expect(document.documentElement.lang).toBe("en");
      await act(async () => {
        resolveInitial({ theme: "light", uiLanguage: "ru", writeSeq: 1 } as AppSettings);
      });
      expect(document.documentElement.dataset.theme).toBe("dark");
      expect(document.documentElement.lang).toBe("en");
    } finally {
      vi.mocked(invoke).mockImplementation(original);
    }
  });

  it("discards a previous take's hover result and cancels with the displayed take's ticket", async () => {
    render(<OverlayApp />);
    const pill = await screen.findByRole("button", { name: /Hover to cancel|Наведите/ });
    let resolvePointer: (matches: boolean) => void = () => undefined;
    pointerCheck.pending = new Promise<boolean>((resolve) => {
      resolvePointer = resolve;
    });
    fireEvent.pointerEnter(pill, { clientX: 12, clientY: 8 });
    await act(async () => {
      overlayHandlers[0]({
        payload: {
          revision: 2,
          generation: 2,
          epoch: 2,
          visible: true,
          state: { kind: "recording" },
        },
      });
    });
    await act(async () => resolvePointer(true));
    expect(pill).not.toHaveClass("overlay-pill-cancel");
    pointerCheck.pending = undefined;
    fireEvent.pointerMove(pill, { clientX: 16, clientY: 9 });
    await waitFor(() => expect(pill).toHaveClass("overlay-pill-cancel"));
    fireEvent.click(pill);
    expect(invoke).toHaveBeenCalledWith("cancel_overlay_dictation", { generation: 2, epoch: 2 });
    expect(invoke).not.toHaveBeenCalledWith("cancel_dictation");
  });

  it("does not arm cancellation without a valid visible-session ticket", async () => {
    render(<OverlayApp />);
    const pill = await screen.findByRole("button", { name: /Hover to cancel|Наведите/ });
    await act(async () => {
      overlayHandlers[0]({
        payload: {
          revision: 2,
          generation: 0,
          epoch: 0,
          visible: true,
          state: { kind: "recording" },
        },
      });
    });
    fireEvent.pointerEnter(pill, { clientX: 12, clientY: 8 });
    fireEvent.click(pill);
    expect(pill).not.toHaveClass("overlay-pill-cancel");
    expect(invoke).not.toHaveBeenCalledWith("cancel_overlay_dictation", expect.anything());
  });
  it("names cancel as cancel and uses theme tokens", async () => {
    render(<OverlayApp />);
    const pill = await screen.findByRole("button", { name: /Hover to cancel|Наведите/ });
    expect(pill).toHaveClass("overlay-pill");
    fireEvent.click(pill);
    expect(invoke).not.toHaveBeenCalledWith("cancel_overlay_dictation", expect.anything());
    fireEvent.pointerEnter(pill, { clientX: 12, clientY: 8 });
    await waitFor(() => expect(pill).toHaveClass("overlay-pill-cancel"));
    expect(pill).toHaveAccessibleName(/Cancel recording|Отменить запись/);
    expect(screen.getByText(/Cancel recording|Отменить запись/)).toHaveClass("overlay-center");
    expect(pill.querySelector(".overlay-wave")).toBeNull();
    expect(pill.querySelector(".overlay-dot")).toBeNull();
    const cancelCallCount = () =>
      vi.mocked(invoke).mock.calls.filter(([command]) => command === "cancel_overlay_dictation")
        .length;
    const callsBeforeClick = cancelCallCount();
    fireEvent.click(pill);
    expect(cancelCallCount()).toBe(callsBeforeClick + 1);
    expect(invoke).toHaveBeenCalledWith("cancel_overlay_dictation", { generation: 1, epoch: 1 });
    fireEvent.keyDown(window, { key: "Escape", bubbles: true });
    expect(invoke).toHaveBeenCalledWith("cancel_dictation");
    expect(cancelCallCount()).toBe(callsBeforeClick + 1);
    fireEvent.pointerLeave(pill);
    expect(pill).not.toHaveClass("overlay-pill-cancel");
    expect(document.documentElement.lang).toBe("en");
  });

  it("does not reopen cancel copy when a new take starts after hide", async () => {
    render(<OverlayApp />);
    const pill = await screen.findByRole("button", { name: /Hover to cancel|Наведите/ });
    fireEvent.pointerEnter(pill, { clientX: 12, clientY: 8 });
    await waitFor(() => expect(pill).toHaveClass("overlay-pill-cancel"));
    await waitFor(() => {
      expect(overlayHandlers.length).toBeGreaterThan(0);
    });
    overlayHandlers[0]({
      payload: { revision: 4, generation: 1, epoch: 1, visible: false, state: { kind: "idle" } },
    });
    await waitFor(() => {
      expect(screen.queryByRole("button")).not.toBeInTheDocument();
    });
    overlayHandlers[0]({
      payload: {
        revision: 5,
        generation: 2,
        epoch: 2,
        visible: true,
        state: { kind: "recording" },
      },
    });
    const next = await screen.findByRole("button", { name: /Hover to cancel|Наведите/ });
    expect(next).not.toHaveClass("overlay-pill-cancel");
    expect(screen.queryByText(/Cancel recording|Отменить запись/)).not.toBeInTheDocument();
  });

  it("drops cancel copy after mouseleave even if :hover was latched", async () => {
    render(<OverlayApp />);
    const pill = await screen.findByRole("button", { name: /Hover to cancel|Наведите/ });
    fireEvent.pointerEnter(pill, { clientX: 12, clientY: 8 });
    await waitFor(() => expect(pill).toHaveClass("overlay-pill-cancel"));
    fireEvent.pointerLeave(pill);
    expect(pill).not.toHaveClass("overlay-pill-cancel");
    expect(screen.queryByText(/Cancel recording|Отменить запись/)).not.toBeInTheDocument();
  });

  it("arms only when the native cursor position matches the pointer event", async () => {
    render(<OverlayApp />);
    const pill = await screen.findByRole("button", { name: /Hover to cancel|Наведите/ });
    pointerCheck.matches = false;
    fireEvent.pointerEnter(pill, { clientX: 12, clientY: 8 });
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("overlay_pointer_matches", { clientX: 12, clientY: 8 }),
    );
    expect(pill).not.toHaveClass("overlay-pill-cancel");
    pointerCheck.matches = true;
    fireEvent.pointerMove(pill, { clientX: 18, clientY: 10 });
    await waitFor(() => expect(pill).toHaveClass("overlay-pill-cancel"));
  });

  it("BUG-HUD-R1 keeps recording until a newer overlay revision arrives", async () => {
    render(<OverlayApp />);
    expect(await screen.findByText("Recording")).toBeInTheDocument();
    await waitFor(() => {
      expect(overlayHandlers.length).toBeGreaterThan(0);
    });
    overlayHandlers[0]({
      payload: {
        revision: 1,
        generation: 1,
        epoch: 1,
        visible: true,
        state: { kind: "transcribing", attempt: 1 },
      },
    });
    expect(screen.getByText("Recording")).toBeInTheDocument();
    overlayHandlers[0]({
      payload: {
        revision: 2,
        generation: 1,
        epoch: 1,
        visible: true,
        state: { kind: "transcribing", attempt: 1 },
      },
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
      payload: { revision: 4, generation: 1, epoch: 1, visible: true, state: { kind: "idle" } },
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
        generation: 1,
        epoch: 1,
        visible: true,
        state: { kind: "recording" },
        limitReached: true,
      },
    });
    expect(await screen.findByText("Recording limit reached")).toBeInTheDocument();
  });
});
