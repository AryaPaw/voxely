import { describe, expect, it } from "vitest";
import { HISTORY_CHANGED, historySearchQuery, shouldReloadHistory } from "./history-sync";

describe("history sync", () => {
  it("session state is not a history persist signal", () => {
    expect(shouldReloadHistory("session://state")).toBe(false);
    expect(shouldReloadHistory(HISTORY_CHANGED)).toBe(true);
  });

  it("keeps the active search query for live reloads", () => {
    expect(historySearchQuery(" needle ")).toBe("needle");
    expect(historySearchQuery("   ")).toBeUndefined();
  });
});
