import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { messagesFor } from "../../../lib/i18n";
import { DebugSettings } from "./DebugSettings";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
}));

afterEach(() => {
  cleanup();
  vi.mocked(invoke).mockClear();
});

describe("DebugSettings", () => {
  it("plays each dictation cue", () => {
    render(<DebugSettings copy={messagesFor("en")} />);
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    fireEvent.click(screen.getByRole("button", { name: "End of recording" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(invoke).toHaveBeenCalledWith("play_cue", { kind: "start" });
    expect(invoke).toHaveBeenCalledWith("play_cue", { kind: "stop" });
    expect(invoke).toHaveBeenCalledWith("play_cue", { kind: "cancel" });
  });

  it("sends a real Windows error notification", () => {
    render(<DebugSettings copy={messagesFor("en")} />);
    fireEvent.click(screen.getByRole("button", { name: "Show notification" }));
    expect(invoke).toHaveBeenCalledWith("preview_error_notification");
  });
});
