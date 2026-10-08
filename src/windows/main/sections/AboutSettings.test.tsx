import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { messagesFor } from "../../../lib/i18n";
import { AboutSettings } from "./AboutSettings";
import { api, type UpdateOutcome } from "../../../lib/api";
import { reportError } from "../../../lib/system-notify";
import { toast } from "sonner";

vi.mock("../../../lib/system-notify", () => ({ reportError: vi.fn() }));

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
  toast: { success: vi.fn(), error: vi.fn(), message: vi.fn() },
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.clearAllMocks();
});

describe("AboutSettings", () => {
  it.each<UpdateOutcome>(["failed", "busy", "deferred", "none"])(
    "announces update outcome %s without offering installation",
    async (outcome) => {
      vi.spyOn(api, "checkForUpdates").mockResolvedValue(outcome);
      render(<AboutSettings copy={messagesFor("en")} />);
      fireEvent.click(await screen.findByRole("button", { name: "Check for updates" }));
      const announce =
        outcome === "failed" ? toast.error : outcome === "none" ? toast.success : toast.message;
      await waitFor(() => expect(announce).toHaveBeenCalledOnce());
      expect(screen.queryByRole("button", { name: "Install update" })).not.toBeInTheDocument();
    },
  );
  it.each<UpdateOutcome>(["failed", "none", "deferred"])(
    "keeps installation available only for retryable outcome %s",
    async (outcome) => {
      vi.spyOn(api, "installUpdate").mockResolvedValue(outcome);
      render(<AboutSettings copy={messagesFor("en")} />);
      fireEvent.click(await screen.findByRole("button", { name: "Check for updates" }));
      fireEvent.click(await screen.findByRole("button", { name: "Install update" }));
      await waitFor(() => expect(api.installUpdate).toHaveBeenCalledOnce());
      await waitFor(() => {
        if (outcome === "none")
          expect(screen.queryByRole("button", { name: "Install update" })).not.toBeInTheDocument();
        else expect(screen.getByRole("button", { name: "Install update" })).toBeEnabled();
      });
    },
  );
  it("reports check and install exceptions, then allows another attempt", async () => {
    vi.spyOn(api, "checkForUpdates")
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValue("available");
    vi.spyOn(api, "installUpdate").mockRejectedValue(new Error("denied"));
    render(<AboutSettings copy={messagesFor("en")} />);
    fireEvent.click(await screen.findByRole("button", { name: "Check for updates" }));
    await waitFor(() => expect(reportError).toHaveBeenCalledOnce());
    expect(screen.getByRole("button", { name: "Check for updates" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    fireEvent.click(await screen.findByRole("button", { name: "Install update" }));
    await waitFor(() => expect(reportError).toHaveBeenCalledTimes(2));
    expect(screen.getByRole("button", { name: "Install update" })).toBeEnabled();
  });
  it("reports link failures from both source and issue actions", async () => {
    const open = vi.spyOn(api, "openGithub").mockRejectedValue(new Error("denied"));
    render(<AboutSettings copy={messagesFor("en")} />);
    fireEvent.click(await screen.findByRole("button", { name: /report a problem/i }));
    fireEvent.click(screen.getByRole("button", { name: /source code/i }));
    await waitFor(() => expect(reportError).toHaveBeenCalledTimes(2));
    expect(open).toHaveBeenCalledWith("issues");
    expect(open).toHaveBeenCalledWith();
  });
  it("shows runtime version, author, GitHub and manual update", async () => {
    const { container } = render(<AboutSettings copy={messagesFor("en")} />);
    expect(await screen.findByText("0.1.0")).toBeInTheDocument();
    expect(container.querySelector('img[src="/favicon.png"]')).toHaveClass(
      "size-20",
      "object-contain",
    );
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
