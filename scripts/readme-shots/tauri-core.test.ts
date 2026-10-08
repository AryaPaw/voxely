import { describe, expect, it } from "vitest";
import { invoke, convertFileSrc } from "./tauri-core";
import { listen } from "./tauri-event";
import type { AppSettings, CompareState, UsageStatistics } from "../../src/lib/api";

describe("browser fixtures", () => {
  it("publishes compare recording and results for the current clip", async () => {
    const changes: unknown[] = [];
    const unsubscribe = await listen("compare://state", ({ payload }) => changes.push(payload));
    await invoke("start_model_compare");
    expect((await invoke<CompareState>("get_model_compare")).recording).toBe(true);
    await invoke("stop_model_compare");
    const result = await invoke<CompareState>("run_model_compare");
    expect(result.slots.length).toBeGreaterThan(1);
    expect(
      result.slots.every((slot) => slot.clipNonce === result.nonce && slot.runId === result.runId),
    ).toBe(true);
    expect(result.listenPath).toMatch(/^data:audio\/wav/);
    expect(changes).toHaveLength(3);
    await invoke("start_model_compare");
    expect((await invoke<CompareState>("get_model_compare")).slots).toEqual([]);
    unsubscribe();
    await invoke("clear_model_compare");
  });
  it("persists settings and emits isolated changed values", async () => {
    const initial = await invoke<AppSettings>("get_settings");
    expect(initial.presets.some((preset) => preset.id === initial.activePresetId)).toBe(true);
    const changes: unknown[] = [];
    const unsubscribe = await listen("settings://changed", ({ payload }) => changes.push(payload));
    await invoke("save_settings", { settings: { ...initial, model: "fixture/model" } });
    const saved = await invoke<AppSettings>("get_settings");
    expect(saved.model).toBe("fixture/model");
    expect(changes).toHaveLength(1);
    saved.model = "mutated consumer";
    expect((await invoke<AppSettings>("get_settings")).model).toBe("fixture/model");
    unsubscribe();
    await invoke("save_settings", { settings: initial });
    expect(changes).toHaveLength(1);
  });
  it("provides a real PCM WAV fixture instead of absent audio paths", () => {
    const source = convertFileSrc("a.wav");
    const bytes = atob(source.split(",")[1].split("#")[0]);
    expect(bytes.slice(0, 4)).toBe("RIFF");
    expect(bytes.slice(8, 12)).toBe("WAVE");
    expect(bytes.length).toBe(3244);
  });
  it("provides internally consistent synthetic usage data for README screenshots", async () => {
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    start.setDate(start.getDate() - 29);
    const end = new Date();
    end.setHours(0, 0, 0, 0);
    end.setDate(end.getDate() + 1);
    const stats = await invoke<UsageStatistics>("get_usage_statistics", {
      start: start.toISOString(),
      end: end.toISOString(),
    });

    expect(stats.daily).toHaveLength(30);
    expect(stats.dictations).toBe(stats.daily.reduce((sum, day) => sum + day.dictations, 0));
    expect(stats.apiRequests).toBe(stats.daily.reduce((sum, day) => sum + day.apiRequests, 0));
    expect(stats.reportedCostUsd).toBeCloseTo(
      stats.daily.reduce((sum, day) => sum + day.reportedCostUsd, 0),
      6,
    );
    expect(stats.models.reduce((sum, model) => sum + model.dictations, 0)).toBe(stats.dictations);
    expect(stats.models.reduce((sum, model) => sum + model.apiRequests, 0)).toBe(stats.apiRequests);
    expect(stats.models.reduce((sum, model) => sum + model.reportedCostUsd, 0)).toBeCloseTo(
      stats.reportedCostUsd,
      6,
    );
    expect(stats.unpricedAttempts).toBeGreaterThan(0);
  });
});
