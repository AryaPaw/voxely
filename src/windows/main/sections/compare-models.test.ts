import { describe, expect, it } from "vitest";
import { COMPARE_MODEL_MAX, compareSlotIds, moveCompareModel } from "./compare-models";

describe("compare-models", () => {
  it("reorders without mutating the source", () => {
    const models = ["a", "b", "c", "d"];
    expect(moveCompareModel(models, 0, 2)).toEqual(["b", "c", "a", "d"]);
    expect(models).toEqual(["a", "b", "c", "d"]);
  });

  it("keeps a hard cap of twelve slots", () => {
    expect(COMPARE_MODEL_MAX).toBe(12);
    expect(compareSlotIds(3)).toEqual(["compare-slot-0", "compare-slot-1", "compare-slot-2"]);
  });
});
