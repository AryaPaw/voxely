import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { messagesFor } from "../../../lib/i18n";
import { ResetWindowButton } from "./AppearanceSettings";

const invoke = vi.fn();

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

vi.mock("../../../lib/system-notify", () => ({
  reportError: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invoke(cmd, args),
}));

afterEach(() => {
  cleanup();
  invoke.mockReset();
});

describe("AppearanceSettings", () => {
  it("resets the main window to the default size", async () => {
    invoke.mockResolvedValue(undefined);
    render(<ResetWindowButton copy={messagesFor("en")} />);
    fireEvent.click(screen.getByRole("button", { name: "Reset window size" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("reset_main_window", undefined);
    });
    expect(toast.success).toHaveBeenCalledWith("Window size reset to the default.");
  });
});
