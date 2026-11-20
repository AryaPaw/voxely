import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { HotkeyCapture } from "./HotkeyCapture";
import { SettingsField } from "./SettingsField";
import { api } from "../../lib/api";

vi.mock("../../lib/api", () => ({
  api: {
    setHotkeyCapture: vi.fn(async () => undefined),
  },
}));

afterEach(() => cleanup());

describe("HotkeyCapture", () => {
  it("resumes global shortcuts when the settings window loses focus", () => {
    const onChange = vi.fn();
    render(<HotkeyCapture value="Ctrl+Shift+Space" prompt="Press keys" onChange={onChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Ctrl+Shift+Space" }));
    expect(api.setHotkeyCapture).toHaveBeenLastCalledWith(true);
    fireEvent.blur(window);
    expect(screen.getByRole("button", { name: "Ctrl+Shift+Space" })).toBeInTheDocument();
    expect(api.setHotkeyCapture).toHaveBeenLastCalledWith(false);
    fireEvent.keyDown(window, { key: "K", ctrlKey: true, code: "KeyK" });
    expect(onChange).not.toHaveBeenCalled();
  });
  it("preserves the field label and help association on its button", () => {
    render(
      <SettingsField label="Global hotkey" description="Choose a recording shortcut">
        <HotkeyCapture value="Ctrl+Shift+Space" prompt="Press keys" onChange={vi.fn()} />
      </SettingsField>,
    );
    const button = screen.getByRole("button", { name: "Global hotkey" });
    expect(screen.getByLabelText("Global hotkey")).toBe(button);
    expect(button).toHaveAccessibleDescription("Choose a recording shortcut");
  });
  it("keeps listening when the parent onChange identity changes", () => {
    const first = vi.fn();
    const { rerender } = render(
      <HotkeyCapture value="Ctrl+Shift+Space" prompt="Press keys" onChange={first} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Ctrl+Shift+Space" }));
    expect(screen.getByRole("button", { name: "Press keys" })).toBeInTheDocument();
    rerender(<HotkeyCapture value="Ctrl+Shift+Space" prompt="Press keys" onChange={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Press keys" })).toBeInTheDocument();
  });

  it("commits a chord and ignores Escape", async () => {
    const onChange = vi.fn();
    render(<HotkeyCapture value="Ctrl+Shift+Space" prompt="Press keys" onChange={onChange} />);
    fireEvent.click(screen.getByRole("button", { name: "Ctrl+Shift+Space" }));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onChange).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Ctrl+Shift+Space" }));
    fireEvent.keyDown(window, { key: "K", ctrlKey: true, code: "KeyK" });
    await waitFor(() => {
      expect(onChange).toHaveBeenCalled();
    });
  });
});
