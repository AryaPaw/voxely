import { describe, expect, it, vi } from "vitest";
import { invoke } from "./native-core";
import type { AppSettings } from "../../src/lib/api";

const { nativeInvoke } = vi.hoisted(() => ({ nativeInvoke: vi.fn(async () => "native") }));
vi.mock("../../node_modules/@tauri-apps/api/core", () => ({
  invoke: nativeInvoke,
  isTauri: () => true,
}));

describe("native screenshot isolation", () => {
  it("keeps settings, audio and credentials out of production IPC", async () => {
    nativeInvoke.mockClear();
    const settings = await invoke<AppSettings>("get_settings");
    expect(settings.uiLanguage).toBe("en");
    expect(await invoke("api_key_configured")).toBe(true);
    await invoke("store_api_key", { key: "demo-only" });
    expect(await invoke("recording_audio_url", { id: "rec-1" })).toMatch(/^data:audio\/wav/);
    await invoke("start_input_meter");
    expect(nativeInvoke).not.toHaveBeenCalled();
  });

  it("uses native IPC for the actual window controls", async () => {
    nativeInvoke.mockClear();
    expect(await invoke("plugin:window|is_maximized", { label: "main" })).toBe("native");
    expect(nativeInvoke).toHaveBeenCalledExactlyOnceWith("plugin:window|is_maximized", {
      label: "main",
    });
  });
});
