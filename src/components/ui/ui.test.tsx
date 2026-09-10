import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { Button } from "./button";
import { Input } from "./input";
import { Select } from "./select";
import { Switch } from "./switch";

afterEach(() => cleanup());

describe("ui primitives", () => {
  it("renders a named button", () => {
    render(<Button>Save key</Button>);
    expect(screen.getByRole("button", { name: "Save key" })).toBeInTheDocument();
  });

  it("exposes switch checked state", () => {
    render(<Switch checked onCheckedChange={() => undefined} aria-label="Automatic retries" />);
    expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "true");
    render(<Switch checked={false} onCheckedChange={() => undefined} aria-label="Off" />);
    expect(screen.getByRole("switch", { name: "Off" })).toHaveAttribute("aria-checked", "false");
  });

  it("renders form controls", () => {
    render(
      <>
        <Input aria-label="Hotkey" />
        <Select aria-label="Theme">
          <option>System</option>
        </Select>
      </>,
    );
    expect(screen.getByLabelText("Hotkey")).toBeInTheDocument();
    expect(screen.getByLabelText("Theme")).toBeInTheDocument();
  });
});
