import { describe, expect, it } from "vitest";
import { acceptSavedSettings, nextWriteSeq, shouldKeepOptimistic } from "./settings-persist";

describe("settings persist protocol", () => {
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
  });
});
