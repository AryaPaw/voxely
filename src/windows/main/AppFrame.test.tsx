import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppFrame } from "./AppFrame";
import { messagesFor } from "../../lib/i18n";
import { reportError } from "../../lib/system-notify";

const state = vi.hoisted(() => ({
  read: vi.fn(),
  listen: vi.fn(),
  release: vi.fn(),
  minimize: vi.fn(),
  toggleMaximize: vi.fn(),
  close: vi.fn(),
  desktop: true,
}));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => state.desktop }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isMaximized: state.read,
    onResized: state.listen,
    minimize: state.minimize,
    toggleMaximize: state.toggleMaximize,
    close: state.close,
  }),
}));
vi.mock("../../lib/system-notify", () => ({ reportError: vi.fn() }));
const props = { copy: messagesFor("en"), version: "0.4.0", localBuild: true, onAbout: vi.fn() };
let resized: () => void;
beforeEach(() => {
  state.desktop = true;
  state.read.mockResolvedValue(false);
  state.listen.mockImplementation(async (callback: typeof resized) => {
    resized = callback;
    return state.release;
  });
  state.minimize.mockResolvedValue(undefined);
  state.toggleMaximize.mockResolvedValue(undefined);
  state.close.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

describe("custom frame", () => {
  it("keeps identity and routes controls through window APIs, including the normal close policy", async () => {
    const view = render(<AppFrame {...props}>Content</AppFrame>);
    expect(screen.getByText("Voxely (Local build)")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Version 0.4.0" })).not.toHaveTextContent(
      "Local build",
    );
    await waitFor(() => expect(state.read).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole("button", { name: "Version 0.4.0" }));
    expect(props.onAbout).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Minimize" }));
    fireEvent.click(screen.getByRole("button", { name: "Maximize" }));
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(state.minimize).toHaveBeenCalledOnce();
    expect(state.toggleMaximize).toHaveBeenCalledOnce();
    expect(state.close).toHaveBeenCalledOnce();
    state.read.mockResolvedValue(true);
    await act(async () => resized());
    expect(screen.getByRole("button", { name: "Restore" })).toBeInTheDocument();
    view.unmount();
    expect(state.release).toHaveBeenCalledOnce();
  });
  it("ignores an older maximize query after a newer resize", async () => {
    const pending: Array<(value: boolean) => void> = [];
    state.read.mockImplementation(() => new Promise<boolean>((resolve) => pending.push(resolve)));
    render(<AppFrame {...props}>Content</AppFrame>);
    await waitFor(() => expect(pending).toHaveLength(1));
    await act(async () => resized());
    await act(async () => pending[1](true));
    await act(async () => pending[0](false));
    expect(screen.getByRole("button", { name: "Restore" })).toBeInTheDocument();
  });
  it("cleans up a late listener without querying a disposed window", async () => {
    let resolve!: (release: () => void) => void;
    state.listen.mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const view = render(<AppFrame {...props}>Content</AppFrame>);
    view.unmount();
    await act(async () => resolve(state.release));
    expect(state.release).toHaveBeenCalledOnce();
    expect(state.read).not.toHaveBeenCalled();
  });
  it("surfaces native failures without hiding the frame or content", async () => {
    state.read.mockRejectedValue(new Error("read failed"));
    render(<AppFrame {...props}>Content</AppFrame>);
    await waitFor(() => expect(reportError).toHaveBeenCalledOnce());
    state.close.mockRejectedValue(new Error("close failed"));
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(reportError).toHaveBeenCalledTimes(2));
    expect(screen.getByText("Content")).toBeInTheDocument();
  });
  it("disables native actions in browser fixtures", () => {
    state.desktop = false;
    render(<AppFrame {...props}>Content</AppFrame>);
    expect(screen.getByRole("button", { name: "Close" })).toBeDisabled();
    expect(state.listen).not.toHaveBeenCalled();
  });
});
