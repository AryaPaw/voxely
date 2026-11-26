import type { CueKind, CuePreset } from "./api";

// The native cue bank packs a start and a shared end clip in this preset order.
export const CUE_PRESETS = ["classic", "soft", "warm"] as const satisfies CuePreset[];
export const CUE_KINDS = ["start", "stop"] as const satisfies CueKind[];

interface CueClip {
  samples: Float32Array;
  durationMs: number;
  peaks: number[];
}

export interface CueBank {
  sampleRate: number;
  presets: Map<CuePreset, Record<CueKind, CueClip>>;
}

export function decodeCueBank(bytes: ArrayBuffer | number[]): CueBank {
  // Tauri's fallback IPC channel delivers raw responses as byte arrays.
  const data = new DataView(Array.isArray(bytes) ? Uint8Array.from(bytes).buffer : bytes);
  const sampleRate = data.getUint32(0, true);
  if (sampleRate < 8_000 || sampleRate > 96_000) throw new Error("Invalid cue sample rate");
  let offset = 4;

  function readClip(): CueClip {
    const length = data.getUint16(offset, true);
    offset += 2;
    if (!length || length > sampleRate / 4 || offset + 2 * length > data.byteLength) {
      throw new Error("Invalid cue clip length");
    }
    const samples = new Float32Array(length);
    for (let index = 0; index < length; index++) {
      samples[index] = data.getInt16(offset, true) / 32767;
      offset += 2;
    }
    const viewLength = Math.round(sampleRate * 0.18);
    const bars = 28;
    const peaks = Array.from({ length: bars }, (_, bar) => {
      const first = Math.floor((bar * viewLength) / bars);
      const end = Math.min(length, Math.floor(((bar + 1) * viewLength) / bars));
      let peak = 0;
      for (let index = first; index < end; index++) peak = Math.max(peak, Math.abs(samples[index]));
      return Math.min(100, (100 * peak) / 0.39);
    });
    return { samples, durationMs: Math.round((1000 * length) / sampleRate), peaks };
  }

  const presets = new Map<CuePreset, Record<CueKind, CueClip>>();
  for (const preset of CUE_PRESETS) {
    presets.set(preset, { start: readClip(), stop: readClip() });
  }
  if (offset !== data.byteLength) throw new Error("Unexpected cue bank data");
  return { sampleRate, presets };
}

export function cueScenario(start: Float32Array, end: Float32Array, sampleRate: number) {
  const endAt = Math.round(0.58 * sampleRate);
  const samples = new Float32Array(endAt + end.length);
  samples.set(start);
  samples.set(end, endAt);
  return samples;
}
