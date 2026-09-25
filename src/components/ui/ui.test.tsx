import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { Button } from "./button";
import { Input } from "./input";
import { Label } from "./label";
import { SimpleSelect } from "./simple-select";
import { Switch } from "./switch";

afterEach(() => cleanup());

describe("ui primitives", () => {
  it("renders a named button", () => {
    render(<Button>Save key</Button>);
    expect(screen.getByRole("button", { name: "Save key" })).toHaveClass("cursor-pointer");
  });

  it("exposes switch checked state", () => {
    render(<Switch checked onCheckedChange={() => undefined} aria-label="Automatic retries" />);
    expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "true");
    render(<Switch checked={false} onCheckedChange={() => undefined} aria-label="Off" />);
    expect(screen.getByRole("switch", { name: "Off" })).toHaveAttribute("aria-checked", "false");
  });

  it("pairs an accessible label with input and select", () => {
    render(
      <>
        <Label htmlFor="hotkey">Hotkey</Label>
        <Input id="hotkey" />
        <SimpleSelect
          aria-label="Theme"
          value="system"
          onValueChange={() => undefined}
          options={[{ value: "system", label: "System" }]}
        />
      </>,
    );
    expect(screen.getByLabelText("Hotkey")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Theme" })).toBeInTheDocument();
  });
});
