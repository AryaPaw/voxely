import { describe, expect, it } from "vitest";
import { HISTORY_CHANGED, shouldReloadHistory } from "./history-sync";

describe("history sync", () => {
  it("session state is not a history persist signal", () => {
    expect(shouldReloadHistory("session://state")).toBe(false);
    expect(shouldReloadHistory(HISTORY_CHANGED)).toBe(true);
  });
});
