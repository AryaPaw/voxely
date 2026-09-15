import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { reportError, sendSystemError } from "./system-notify";

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
  },
}));

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: { title: string; body: string }) => invoke(cmd, args),
}));

describe("sendSystemError", () => {
  beforeEach(() => {
    invoke.mockReset();
    vi.mocked(toast.error).mockReset();
  });

  it("sends a Windows toast through the native host", async () => {
    invoke.mockResolvedValue(undefined);
    await expect(sendSystemError("Voxely", "Нет сети")).resolves.toBe(true);
    expect(invoke).toHaveBeenCalledWith("show_system_notification", {
      title: "Voxely",
      body: "Нет сети",
    });
  });

  it("swallows plugin failures in tests and the web shell", async () => {
    invoke.mockRejectedValue(new Error("no backend"));
    await expect(sendSystemError("Voxely", "fail")).resolves.toBe(false);
  });
});

describe("reportError", () => {
  beforeEach(() => {
    invoke.mockResolvedValue(undefined);
    vi.mocked(toast.error).mockReset();
  });

  it("shows an in-app toast and a system toast", async () => {
    reportError("Не удалось сохранить данные");
    expect(toast.error).toHaveBeenCalledWith("Не удалось сохранить данные");
    await vi.waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("show_system_notification", {
        title: "Voxely",
        body: "Не удалось сохранить данные",
      });
    });
  });
});
