import { act, cleanup, render } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { api, type AppSettings } from "../../../lib/api";
import { messagesFor } from "../../../lib/i18n";
import { AudioSettings } from "./AudioSettings";

const events = vi.hoisted(() => ({ handlers: [] as Array<() => void> }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_event: string, handler: () => void) => {
    events.handlers.push(handler);
    return () => undefined;
  }),
}));
vi.mock("../../../lib/api", () => ({
  api: {
    mics: vi.fn(async () => []),
    meter: vi.fn(async () => ({ peak: 0, rms: 0 })),
    startInputMeter: vi.fn(),
    stopInputMeter: vi.fn(async () => undefined),
  },
}));

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.clearAllMocks();
  events.handlers.length = 0;
});
const props = {
  settings: { inputDevice: "default" } as AppSettings,
  copy: messagesFor("en"),
  onChange: vi.fn(),
};

it("single-flights session events and closes a late start only with its original owner", async () => {
  let finishOld!: (started: boolean) => void;
  vi.mocked(api.startInputMeter)
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishOld = resolve;
        }),
    )
    .mockResolvedValue(true);
  const oldView = render(<AudioSettings {...props} />);
  await act(async () => {
    events.handlers[0]();
    events.handlers[0]();
  });
  expect(api.startInputMeter).toHaveBeenCalledTimes(1);
  const oldOwner = vi.mocked(api.startInputMeter).mock.calls[0][0];
  oldView.unmount();
  render(<AudioSettings {...props} />);
  await act(async () => {});
  const newOwner = vi.mocked(api.startInputMeter).mock.calls[1][0];
  expect(newOwner).not.toBe(oldOwner);
  await act(async () => {
    finishOld(true);
  });
  expect(api.stopInputMeter).toHaveBeenLastCalledWith(oldOwner);
  const before = vi.mocked(api.startInputMeter).mock.calls.length;
  await act(async () => {
    events.handlers[0]();
  });
  expect(api.startInputMeter).toHaveBeenCalledTimes(before);
});

it("retries admission after a busy start without overlapping pending requests", async () => {
  vi.useFakeTimers();
  vi.mocked(api.startInputMeter).mockResolvedValueOnce(false).mockResolvedValue(true);
  const view = render(<AudioSettings {...props} />);
  await act(async () => {});
  await act(async () => {
    await vi.advanceTimersByTimeAsync(120);
  });
  expect(api.startInputMeter).toHaveBeenCalledTimes(2);
  expect(vi.mocked(api.startInputMeter).mock.calls[0][0]).toBe(
    vi.mocked(api.startInputMeter).mock.calls[1][0],
  );
  view.unmount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(500);
  });
  expect(api.startInputMeter).toHaveBeenCalledTimes(2);
});

it("backs off rejected microphone meter starts", async () => {
  vi.useFakeTimers();
  vi.mocked(api.startInputMeter)
    .mockRejectedValueOnce(new Error("microphone unavailable"))
    .mockResolvedValue(true);
  const view = render(<AudioSettings {...props} />);
  await act(async () => {});
  expect(api.startInputMeter).toHaveBeenCalledTimes(1);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(599);
  });
  expect(api.startInputMeter).toHaveBeenCalledTimes(1);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1);
  });
  expect(api.startInputMeter).toHaveBeenCalledTimes(2);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(500);
  });
  expect(api.startInputMeter).toHaveBeenCalledTimes(2);
  view.unmount();
});
