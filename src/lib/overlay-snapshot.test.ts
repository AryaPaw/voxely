import { describe, expect, it } from "vitest";
import type { OverlaySnapshot } from "./api";
import { acceptOverlayRevision, applyOverlaySnapshot } from "./overlay-snapshot";
import { overlayHudLabel } from "./session-copy";

describe("applyOverlaySnapshot", () => {
  it("BUG-HUD-R1 accepts a newer revision when HUD status changes without hide", () => {
    const recording: OverlaySnapshot = {
      revision: 1,
      visible: true,
      state: { kind: "recording" },
    };
    const transcribing: OverlaySnapshot = {
      revision: 2,
      visible: true,
      state: { kind: "transcribing", attempt: 1 },
    };
    const applied = applyOverlaySnapshot(recording, transcribing);
    expect(applied.state.kind).toBe("transcribing");
    expect(overlayHudLabel(applied.visible, applied.state)).toBe("Расшифровка");
  });

  it("BUG-HUD-R1 ignores same-revision status so stale events cannot rewind HUD", () => {
    const recording: OverlaySnapshot = {
      revision: 4,
      visible: true,
      state: { kind: "recording" },
    };
    const stale: OverlaySnapshot = {
      revision: 4,
      visible: true,
      state: { kind: "transcribing", attempt: 1 },
    };
    expect(acceptOverlayRevision(4, 4)).toBe(false);
    expect(applyOverlaySnapshot(recording, stale).state.kind).toBe("recording");
  });
});
