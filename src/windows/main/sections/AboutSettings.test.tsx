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
    render(<AboutSettings copy={messagesFor("en")} />);
    expect(await screen.findByText(/Version: 0\.1\.0 \(.+2026\)/)).toBeInTheDocument();
    expect(screen.getByText("AryaPaw")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "AryaPaw/voxely" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Issues" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check for updates" })).toBeInTheDocument();
  });
});
