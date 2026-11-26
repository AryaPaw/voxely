import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { api } from "../../../lib/api";
import { messagesFor } from "../../../lib/i18n";
import { SoundCueSettings } from "./SoundCueSettings";

vi.mock("../../../lib/api", () => ({ api: { cuePreviews: vi.fn() } }));

const rate = 8_000;
const copy = messagesFor("en");

function cuePacket() {
  const frameCount = 80;
  const bytes = new ArrayBuffer(4 + 6 * (2 + frameCount * 2));
  const data = new DataView(bytes);
  data.setUint32(0, rate, true);
  for (let index = 0; index < 6; index++) {
    const offset = 4 + index * (2 + frameCount * 2);
    data.setUint16(offset, frameCount, true);
    data.setInt16(offset + 2, 1024 * (index + 1), true);
  }
  return bytes;
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.resetAllMocks();
});

it("shows the three current sets and asks to persist the selected preset", async () => {
  vi.mocked(api.cuePreviews).mockResolvedValue(cuePacket());
  const onChange = vi.fn();
  const props = { copy, onChange };
  const view = render(<SoundCueSettings selected="classic" {...props} />);
  expect(screen.getByRole("radio", { name: "Classic" })).toHaveAttribute("aria-checked", "true");
  const warmCue = screen.getByRole("button", { name: "Warm – Start" });
  await waitFor(() => expect(warmCue).toBeEnabled());
  expect(warmCue.querySelector("[data-waveform]")?.childElementCount).toBe(28);
  expect(screen.getAllByRole("radio")).toHaveLength(3);
  expect(screen.queryByRole("radio", { name: "Short" })).not.toBeInTheDocument();
  expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Stop playback" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Warm – Cancel" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Warm – End / cancel" })).toBeEnabled();
  fireEvent.click(screen.getByRole("radio", { name: "Warm" }));
  expect(onChange).toHaveBeenCalledWith("warm");
  view.rerender(<SoundCueSettings selected="warm" {...props} />);
  expect(screen.getByRole("radio", { name: "Warm" })).toHaveAttribute("aria-checked", "true");
});

it("plays the shared end cue in the sequence and replaces the previous preview", async () => {
  vi.mocked(api.cuePreviews).mockResolvedValue(cuePacket());
  const playback: Float32Array[] = [];
  const stop = vi.fn();
  const close = vi.fn(async () => {});
  function gainNode() {
    return { gain: { setTargetAtTime: vi.fn(), value: 1 }, connect: vi.fn(), disconnect: vi.fn() };
  }
  vi.stubGlobal(
    "AudioContext",
    class {
      currentTime = 0;
      destination = {};
      createGain = gainNode;
      close = close;
      resume = vi.fn(async () => {});
      createBuffer(_channels: number, frames: number, sampleRate: number) {
        expect(sampleRate).toBe(rate);
        const samples = new Float32Array(frames);
        return { getChannelData: () => samples, samples };
      }
      createBufferSource() {
        return {
          buffer: null as { samples: Float32Array } | null,
          connect: vi.fn(),
          disconnect: vi.fn(),
          stop,
          start() {
            playback.push(this.buffer!.samples);
          },
        };
      }
    },
  );
  const view = render(<SoundCueSettings copy={copy} selected="soft" onChange={vi.fn()} />);
  const start = screen.getByRole("button", { name: "Soft – Start" });
  await waitFor(() => expect(start).toBeEnabled());
  await act(async () => {
    fireEvent.click(start);
  });
  expect(playback[0][0]).toBeCloseTo(3072 / 32767);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: `Soft – ${copy.cueScenarioStop}` }));
  });
  expect(stop).toHaveBeenCalledOnce();
  expect(playback[1].length).toBe(0.58 * rate + 80);
  expect(playback[1][0]).toBeCloseTo(3072 / 32767);
  expect(playback[1][100]).toBe(0);
  expect(playback[1][0.58 * rate]).toBeCloseTo(4096 / 32767);
  view.unmount();
  expect(close).toHaveBeenCalledOnce();
});

it("allows recovery after a malformed response without saving any selection", async () => {
  vi.mocked(api.cuePreviews)
    .mockResolvedValueOnce(cuePacket().slice(0, 8))
    .mockResolvedValueOnce(Array.from(new Uint8Array(cuePacket())));
  const onChange = vi.fn();
  render(<SoundCueSettings copy={copy} selected="classic" onChange={onChange} />);
  expect(await screen.findByRole("alert")).toHaveTextContent(copy.cueLoadError);
  fireEvent.click(screen.getByRole("button", { name: copy.cueReload }));
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Classic – Start" })).toBeEnabled(),
  );
  expect(onChange).not.toHaveBeenCalled();
});
