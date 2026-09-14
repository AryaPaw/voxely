import { describe, expect, it } from "vitest";
import { HISTORY_CHANGED, shouldReloadHistory } from "./history-sync";

describe("history sync", () => {
  it("reloads on persisted history events only", () => {
    expect(shouldReloadHistory("session://state")).toBe(false);
    expect(shouldReloadHistory(HISTORY_CHANGED)).toBe(true);
  });
});
