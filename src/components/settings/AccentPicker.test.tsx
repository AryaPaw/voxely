import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AccentPicker } from "./AccentPicker";
import { accentPreset } from "../../lib/accent";
import { contrast } from "../../lib/accent-color";
afterEach(cleanup);
it("shows the default choice and changes family and shade without discarding the other value", () => {
  const onChange = vi.fn();
  const { rerender } = render(
    <AccentPicker onChange={onChange} label="Color" shadeLabel="Shade" resetLabel="Reset color" />,
  );
  expect(screen.getByRole("button", { name: "sky" })).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(screen.getByRole("button", { name: "red" }));
  expect(onChange).toHaveBeenLastCalledWith({ family: "red", shade: 500 });
  rerender(
    <AccentPicker
      value={{ family: "red", shade: 500 }}
      onChange={onChange}
      label="Color"
      shadeLabel="Shade"
      resetLabel="Reset color"
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "600" }));
  expect(onChange).toHaveBeenLastCalledWith({ family: "red", shade: 600 });
});

it("previews each actual shade with readable text and keeps its fill unchanged on hover", () => {
  const onChange = vi.fn();
  const { rerender } = render(
    <AccentPicker
      value={{ family: "blue", shade: 600 }}
      onChange={onChange}
      label="Color"
      shadeLabel="Shade"
      resetLabel="Reset color"
    />,
  );
  for (const shade of [400, 500, 600]) {
    const button = screen.getByRole("button", { name: String(shade) });
    const background = button.style.getPropertyValue("--primary");
    const foreground = button.style.getPropertyValue("--primary-foreground");
    expect(background).toBe(accentPreset({ family: "blue", shade }).color);
    expect(contrast(background, foreground)).toBeGreaterThanOrEqual(4.5);
    expect(button.style.backgroundColor).toBe("");
    expect(button.style.getPropertyValue("--primary-hover")).toBe(background);
  }
  expect(
    screen.getByRole("button", { name: "600" }).style.getPropertyValue("--primary-foreground"),
  ).toBe("#ffffff");
  rerender(
    <AccentPicker
      value={{ family: "sky", shade: 400 }}
      onChange={onChange}
      label="Color"
      shadeLabel="Shade"
      resetLabel="Reset color"
    />,
  );
  expect(
    screen.getByRole("button", { name: "400" }).style.getPropertyValue("--primary-foreground"),
  ).toBe("#000000");
});

it("resets both family and shade to the canonical default and disables reset at that default", () => {
  const onChange = vi.fn();
  const { rerender } = render(
    <AccentPicker
      value={{ family: "blue", shade: 600 }}
      onChange={onChange}
      label="Color"
      shadeLabel="Shade"
      resetLabel="Reset color"
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Reset color" }));
  expect(onChange).toHaveBeenCalledExactlyOnceWith({ family: "sky", shade: 500 });
  rerender(
    <AccentPicker
      value={{ family: "sky", shade: 600 }}
      onChange={onChange}
      label="Color"
      shadeLabel="Shade"
      resetLabel="Reset color"
    />,
  );
  expect(screen.getByRole("button", { name: "Reset color" })).toBeEnabled();
  rerender(
    <AccentPicker onChange={onChange} label="Color" shadeLabel="Shade" resetLabel="Reset color" />,
  );
  expect(screen.getByRole("button", { name: "Reset color" })).toBeDisabled();
});
