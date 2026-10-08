import { describe, expect, it } from "vitest";
import { acceptSavedSettings, nextWriteSeq, shouldKeepOptimistic } from "./settings-persist";

describe("settings persist protocol", () => {
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
