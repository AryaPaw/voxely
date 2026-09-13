import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { messagesFor } from "../../../lib/i18n";
import { AboutSettings } from "./AboutSettings";

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(async () => "0.1.0"),
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
}));

afterEach(() => cleanup());

describe("AboutSettings", () => {
  it("shows runtime version, author, GitHub and manual update", async () => {
    render(<AboutSettings copy={messagesFor("en")} localBuild={false} />);
    expect(await screen.findByText("0.1.0")).toBeInTheDocument();
    expect(screen.getByText(/2026/)).toBeInTheDocument();
    expect(screen.getByText("Voxely")).toBeInTheDocument();
    expect(screen.getByText("Voice dictation into any text field")).toBeInTheDocument();
    expect(screen.getByText("AryaPaw")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /source code/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /AryaPaw\/voxely/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /report a problem/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Issues$/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check for updates" })).toBeInTheDocument();
  });

  it("labels the issue action in Russian", async () => {
    render(<AboutSettings copy={messagesFor("ru")} localBuild={false} />);
    expect(await screen.findByText("0.1.0")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /сообщить о проблеме/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /исходный код/i })).toBeInTheDocument();
  });
});
