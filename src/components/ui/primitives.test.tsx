import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "./card";
import { Slider } from "./slider";
import { ScrollArea } from "./scroll-area";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "./tabs";
import { toast } from "sonner";
import { Toaster } from "./sonner";

afterEach(() => {
  cleanup();
  toast.dismiss();
  document.documentElement.classList.remove("dark");
});
describe("composed primitives", () => {
  it("preserves card semantics, custom classes and action behaviour", () => {
    const save = vi.fn();
    const { container } = render(
      <Card size="sm" className="custom-card">
        <CardHeader>
          <CardTitle role="heading">Storage</CardTitle>
          <CardDescription>Local recordings</CardDescription>
          <CardAction>
            <button onClick={save}>Save</button>
          </CardAction>
        </CardHeader>
        <CardContent>1 GB</CardContent>
        <CardFooter>Stored locally</CardFooter>
      </Card>,
    );
    expect(screen.getByRole("heading", { name: "Storage" })).toBeInTheDocument();
    expect(container.querySelector('[data-slot="card"]')).toHaveAttribute("data-size", "sm");
    expect(container.querySelector('[data-slot="card"]')).toHaveClass("custom-card");
    expect(screen.getByText("Local recordings")).toBeInTheDocument();
    expect(screen.getByText("1 GB")).toHaveAttribute("data-slot", "card-content");
    expect(screen.getByText("Stored locally")).toHaveAttribute("data-slot", "card-footer");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(save).toHaveBeenCalledOnce();
  });
  it("lets keyboard users change an uncontrolled slider", () => {
    const changed = vi.fn();
    render(<Slider aria-label="Gain" defaultValue={[20]} onValueChange={changed} />);
    const thumb = screen.getByRole("slider", { name: "Gain" });
    expect(thumb).toHaveAttribute("aria-valuenow", "20");
    fireEvent.keyDown(thumb, { key: "ArrowRight" });
    expect(changed).toHaveBeenCalledWith([21]);
    expect(thumb).toHaveAttribute("aria-valuenow", "21");
  });
  it("provides bounded range thumbs when no values are supplied", () => {
    const { container } = render(<Slider min={5} max={10} />);
    expect(container.querySelectorAll('[data-slot="slider-thumb"]')).toHaveLength(2);
  });
  it("keeps scrollable content available in its viewport", () => {
    const { container } = render(
      <ScrollArea aria-label="Recordings">
        <p>First recording</p>
      </ScrollArea>,
    );
    expect(screen.getByText("First recording")).toBeInTheDocument();
    expect(container.querySelector('[data-slot="scroll-area-viewport"]')).toContainElement(
      screen.getByText("First recording"),
    );
  });
  it("switches tabs without exposing inactive panels", () => {
    render(
      <Tabs defaultValue="one">
        <TabsList aria-label="View">
          <TabsTrigger value="one">History</TabsTrigger>
          <TabsTrigger value="two">Statistics</TabsTrigger>
        </TabsList>
        <TabsContent value="one">Recording list</TabsContent>
        <TabsContent value="two">Usage totals</TabsContent>
      </Tabs>,
    );
    expect(screen.getByRole("tabpanel")).toHaveTextContent("Recording list");
    fireEvent.mouseDown(screen.getByRole("tab", { name: "Statistics" }), {
      button: 0,
      ctrlKey: false,
    });
    expect(screen.getByRole("tabpanel")).toHaveTextContent("Usage totals");
    expect(screen.getByRole("tab", { name: "Statistics" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });
  it.each(["light", "dark", "explicit"])("resolves toaster theme for %s", async (theme) => {
    if (theme === "dark") document.documentElement.classList.add("dark");
    const { container } = render(
      <Toaster {...(theme === "explicit" ? { theme: "dark" as const } : {})} />,
    );
    act(() => {
      toast("Theme check");
    });
    await screen.findByText("Theme check");
    expect(container.querySelector("[data-sonner-toaster]")).toHaveAttribute(
      "data-sonner-theme",
      theme === "light" ? "light" : "dark",
    );
  });
});
