import { vi } from "vitest";
import "@testing-library/jest-dom/vitest";

vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: vi.fn(async () => true),
  requestPermission: vi.fn(async () => "granted"),
  sendNotification: vi.fn(),
}));

class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = ResizeObserverStub as unknown as typeof ResizeObserver;
}

HTMLCanvasElement.prototype.getContext = function getContext() {
  return {
    clearRect: () => undefined,
    fill: () => undefined,
    beginPath: () => undefined,
    roundRect: () => undefined,
    rect: () => undefined,
    setTransform: () => undefined,
    fillStyle: "",
  };
} as unknown as typeof HTMLCanvasElement.prototype.getContext;

HTMLAudioElement.prototype.play = function play() {
  return Promise.resolve();
};
HTMLAudioElement.prototype.pause = function pause() {
  return undefined;
};
