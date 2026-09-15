import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { messagesFor } from "../../../lib/i18n";
import { AboutSettings } from "./AboutSettings";

const invoke = vi.fn(async (cmd: string) => {
  if (cmd === "get_runtime_info") {
    return { localBuild: false, buildDate: "2026-09-15" };
  }
  if (cmd === "check_for_updates") {
    return "available";
  }
  if (cmd === "install_update") {
    return "installed";
  }
  return undefined;
});

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(async () => "0.1.0"),
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string) => invoke(cmd),
}));
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

afterEach(() => cleanup());

describe("AboutSettings", () => {
  it("shows runtime version, author, GitHub and manual update", async () => {
    render(<AboutSettings copy={messagesFor("en")} />);
    expect(await screen.findByText("0.1.0")).toBeInTheDocument();
    expect(screen.getByText(/2026/)).toBeInTheDocument();
    expect(screen.getByText("Voxely")).toBeInTheDocument();
    expect(screen.getByText("Voice dictation into any text field")).toBeInTheDocument();
    expect(screen.getByText("AryaPaw")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /source code/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /AryaPaw\/voxely/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /report a problem/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Issues$/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check for updates" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Install update" })).not.toBeInTheDocument();
  });

  it("offers install after a manual check finds an update", async () => {
    render(<AboutSettings copy={messagesFor("en")} />);
    fireEvent.click(await screen.findByRole("button", { name: "Check for updates" }));
    expect(await screen.findByRole("button", { name: "Install update" })).toBeInTheDocument();
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("check_for_updates");
    });
    expect(invoke).not.toHaveBeenCalledWith("install_update");
    fireEvent.click(screen.getByRole("button", { name: "Install update" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("install_update");
    });
  });

  it("labels the issue action in Russian", async () => {
    render(<AboutSettings copy={messagesFor("ru")} />);
    expect(await screen.findByText("0.1.0")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /сообщить о проблеме/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /исходный код/i })).toBeInTheDocument();
  });
});
