import { describe, expect, it, vi } from "vitest";
import { invokeWhenManaged, isStateNotManagedError } from "./invoke-ready";

describe("isStateNotManagedError", () => {
  it("matches the Tauri State extractor text", () => {
    expect(
      isStateNotManagedError(
        "state not managed for field 'ctx' on command 'get_settings'. You must call `.manage()` before using this command",
      ),
    ).toBe(true);
    expect(
      isStateNotManagedError({
        message: "state not managed for field 'ctx' on command 'get_settings'",
      }),
    ).toBe(true);
    expect(isStateNotManagedError("StorageFailed")).toBe(false);
  });
});

describe("invokeWhenManaged", () => {
  it("retries until ctx is managed", async () => {
    const run = vi
      .fn()
      .mockRejectedValueOnce("state not managed for field 'ctx' on command 'get_settings'")
      .mockResolvedValueOnce({ theme: "light" });
    await expect(invokeWhenManaged(run, "get_settings")).resolves.toEqual({ theme: "light" });
    expect(run).toHaveBeenCalledTimes(2);
  });

  it("does not retry unrelated failures", async () => {
    const run = vi.fn().mockRejectedValue("StorageFailed");
    await expect(invokeWhenManaged(run, "get_settings")).rejects.toBe("StorageFailed");
    expect(run).toHaveBeenCalledTimes(1);
  });
});
