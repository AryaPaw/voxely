import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { reportError, sendSystemError } from "./system-notify";

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
  },
}));

const isPermissionGranted = vi.fn();
const requestPermission = vi.fn();
const sendNotification = vi.fn();

vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: () => isPermissionGranted(),
  requestPermission: () => requestPermission(),
  sendNotification: (payload: { title: string; body: string }) => sendNotification(payload),
}));

describe("sendSystemError", () => {
  beforeEach(() => {
    isPermissionGranted.mockReset();
    requestPermission.mockReset();
    sendNotification.mockReset();
    vi.mocked(toast.error).mockReset();
  });

  it("sends a Windows toast when permission is already granted", async () => {
    isPermissionGranted.mockResolvedValue(true);
    await expect(sendSystemError("Voxely", "Нет сети")).resolves.toBe(true);
    expect(sendNotification).toHaveBeenCalledWith({ title: "Voxely", body: "Нет сети" });
    expect(requestPermission).not.toHaveBeenCalled();
  });

  it("requests permission once when it is missing", async () => {
    isPermissionGranted.mockResolvedValue(false);
    requestPermission.mockResolvedValue("granted");
    await expect(sendSystemError("Voxely", "fail")).resolves.toBe(true);
    expect(requestPermission).toHaveBeenCalled();
    expect(sendNotification).toHaveBeenCalled();
  });

  it("skips the toast when permission is denied", async () => {
    isPermissionGranted.mockResolvedValue(false);
    requestPermission.mockResolvedValue("denied");
    await expect(sendSystemError("Voxely", "fail")).resolves.toBe(false);
    expect(sendNotification).not.toHaveBeenCalled();
  });

  it("swallows plugin failures in tests and the web shell", async () => {
    isPermissionGranted.mockRejectedValue(new Error("no backend"));
    await expect(sendSystemError("Voxely", "fail")).resolves.toBe(false);
  });
});

describe("reportError", () => {
  beforeEach(() => {
    isPermissionGranted.mockResolvedValue(true);
    sendNotification.mockReset();
    vi.mocked(toast.error).mockReset();
  });

  it("shows an in-app toast and a system toast", async () => {
    reportError("Не удалось сохранить данные");
    expect(toast.error).toHaveBeenCalledWith("Не удалось сохранить данные");
    await vi.waitFor(() => {
      expect(sendNotification).toHaveBeenCalledWith({
        title: "Voxely",
        body: "Не удалось сохранить данные",
      });
    });
  });
});
