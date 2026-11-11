import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { api, type AppSettings } from "../../../lib/api";
import { messagesFor } from "../../../lib/i18n";
import {
  AudioSettings as CompleteAudioSettings,
  MicrophoneSettings as AudioSettings,
} from "./AudioSettings";

const events = vi.hoisted(() => ({
  handlers: [] as Array<() => void>,
  byName: new Map<string, Array<(event: { payload: boolean }) => void>>(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, handler: (event: { payload: boolean }) => void) => {
    if (event === "session://state") events.handlers.push(() => handler({ payload: false }));
    events.byName.set(event, [...(events.byName.get(event) ?? []), handler]);
    return () => undefined;
  }),
}));
vi.mock("../../../lib/api", () => ({
  api: {
    mics: vi.fn(async () => []),
    meter: vi.fn(async () => ({ peak: 0, rms: 0 })),
    previewDsp: vi.fn(async () => null),
    startFilterSample: vi.fn(async () => undefined),
    stopFilterSample: vi.fn(async () => ({
      originalPath: "original",
      processedPath: "processed",
      nonce: 1,
      peak: 0,
      rms: 0,
      clipCount: 0,
    })),
    startInputMeter: vi.fn(),
    stopInputMeter: vi.fn(async () => undefined),
  },
}));

vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc: (path: string) => path }));

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.clearAllMocks();
  events.handlers.length = 0;
  events.byName.clear();
});
const props = {
  settings: { inputDevice: "default", hotkey: "Ctrl+Shift+Space" } as AppSettings,
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

it.each([false, true])(
  "resumes the microphone meter after a sample settles (start failure: %s)",
  async (failStart) => {
    const starts = vi.mocked(api.startInputMeter).mockResolvedValue(true);
    const emit = (active: boolean) =>
      events.byName.get("filter://sample")?.forEach((handler) => handler({ payload: active }));
    vi.mocked(api.startFilterSample).mockImplementation(async () => {
      // Matches the real backend: release the meter, then publish success or failed-start release.
      emit(!failStart);
      if (failStart) throw { code: "AudioCaptureFailed" };
    });
    vi.mocked(api.stopFilterSample).mockImplementation(async () => {
      emit(false);
      return {
        originalPath: "original",
        processedPath: "processed",
        nonce: 1,
        peak: 0,
        rms: 0,
        clipCount: 0,
      };
    });
    render(
      <CompleteAudioSettings
        {...props}
        settings={{
          ...props.settings,
          presets: [],
          activePresetId: "",
          silenceCompression: { enabled: true, maxPauseMs: 2000 },
        }}
      />,
    );
    await waitFor(() => expect(starts).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole("button", { name: props.copy.recordSample }));
    if (!failStart) {
      await screen.findByRole("button", { name: props.copy.stopSample });
      expect(starts).toHaveBeenCalledOnce();
      fireEvent.click(screen.getByRole("button", { name: props.copy.stopSample }));
    }
    await waitFor(() => expect(starts).toHaveBeenCalledTimes(2));
    expect(starts.mock.calls[1][0]).toBe(starts.mock.calls[0][0]);
  },
);

it("rejects a stale successful meter start after sample ownership changed", async () => {
  vi.useFakeTimers();
  let release!: (value: boolean) => void;
  const starts = vi
    .mocked(api.startInputMeter)
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    )
    .mockResolvedValue(true);
  render(<AudioSettings {...props} />);
  const owner = starts.mock.calls[0][0];
  await act(async () => {
    events.byName.get("filter://sample")?.forEach((handler) => handler({ payload: true }));
    events.byName.get("filter://sample")?.forEach((handler) => handler({ payload: false }));
  });
  expect(starts).toHaveBeenCalledOnce();
  await act(async () => {
    release(true);
  });
  expect(api.stopInputMeter).toHaveBeenCalledWith(owner);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(120);
  });
  expect(starts).toHaveBeenCalledTimes(2);
  expect(starts.mock.calls[1][0]).toBe(owner);
});

it("clears a recovered meter error without hiding device enumeration failure", async () => {
  vi.useFakeTimers();
  vi.mocked(api.mics).mockRejectedValueOnce(new Error("device enumeration failed"));
  vi.mocked(api.startInputMeter)
    .mockRejectedValueOnce(new Error("temporary microphone failure"))
    .mockResolvedValue(true);
  render(<AudioSettings {...props} />);
  await act(async () => {});
  expect(screen.getByText("device enumeration failed")).toBeInTheDocument();
  expect(screen.getByText("temporary microphone failure")).toBeInTheDocument();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(600);
  });
  expect(screen.queryByText("temporary microphone failure")).not.toBeInTheDocument();
  expect(screen.getByText("device enumeration failed")).toBeInTheDocument();
});

it("clears a previous device-list error when the next localized request succeeds", async () => {
  vi.mocked(api.mics).mockRejectedValueOnce(new Error("device enumeration failed"));
  vi.mocked(api.startInputMeter).mockResolvedValue(true);
  const view = render(<AudioSettings {...props} />);
  await screen.findByText("device enumeration failed");
  view.rerender(<AudioSettings {...props} copy={messagesFor("ru")} />);
  await waitFor(() =>
    expect(screen.queryByText("device enumeration failed")).not.toBeInTheDocument(),
  );
});
