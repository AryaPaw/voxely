import { describe, expect, it } from "vitest";
import {
  acceptSavedSettings,
  nextWriteSeq,
  reconcileFailedSettings,
  shouldKeepOptimistic,
} from "./settings-persist";

describe("settings persist protocol", () => {
  it("restores an older persisted snapshot after its optimistic write failed", () => {
    const persisted = { writeSeq: 1, theme: "dark" };
    expect(reconcileFailedSettings({ writeSeq: 2, theme: "light" }, 2, persisted)).toBe(persisted);
    expect(reconcileFailedSettings(null, 2, persisted)).toBe(persisted);
  });

  it("preserves a later draft while accepting newer authoritative rollback events", () => {
    const draft = { writeSeq: 3, theme: "light" };
    expect(reconcileFailedSettings(draft, 2, { writeSeq: 1, theme: "dark" })).toBe(draft);
    expect(reconcileFailedSettings(draft, 2, { writeSeq: 4, theme: "system" })).toEqual({
      writeSeq: 4,
      theme: "system",
    });
  });
  it("accepts legacy snapshots without sequence metadata and preserves newer drafts", () => {
    const legacy: { writeSeq?: number; model: string } = { model: "legacy" };
    expect(acceptSavedSettings(null, legacy)).toBe(legacy);
    expect(acceptSavedSettings(legacy, { model: "saved" })).toEqual({ model: "saved" });
    expect(acceptSavedSettings({ writeSeq: 2, model: "new" }, legacy)).toEqual({
      writeSeq: 2,
      model: "new",
    });
    expect(shouldKeepOptimistic(legacy, 0)).toBe(false);
  });
  it("ignores an older saved snapshot after a newer optimistic write", () => {
    const first = { writeSeq: 1, model: "a" };
    const second = { writeSeq: 2, model: "b" };
    expect(acceptSavedSettings(second, first)).toEqual(second);
    expect(acceptSavedSettings(first, second)).toEqual(second);
  });

  it("does not roll back a newer draft when an older write fails", () => {
    expect(nextWriteSeq(3)).toBe(4);
    expect(shouldKeepOptimistic({ writeSeq: 5 }, 4)).toBe(true);
    expect(shouldKeepOptimistic({ writeSeq: 4 }, 4)).toBe(false);
    expect(shouldKeepOptimistic(null, 1)).toBe(false);
    expect(acceptSavedSettings(null, { writeSeq: 1 })).toEqual({ writeSeq: 1 });
    expect(acceptSavedSettings({ writeSeq: 1 }, { writeSeq: 0 })).toEqual({ writeSeq: 1 });
  });
});
