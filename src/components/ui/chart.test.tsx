import type { ComponentProps, ReactNode } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  ChartContainer,
  ChartLegendContent,
  ChartStyle,
  ChartTooltipContent,
  type ChartConfig,
} from "./chart";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
    width: 320,
    height: 200,
    top: 0,
    left: 0,
    bottom: 200,
    right: 320,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
const config: ChartConfig = {
  duration: { label: "Duration", color: "#123456" },
  words: { label: "Words", theme: { light: "#ffffff", dark: "#000000" } },
  voiced: { label: "Voiced", icon: () => <svg aria-label="Voice series" /> },
};
function chart(children: ReactNode, chartConfig = config) {
  return render(
    <ChartContainer id="usage" config={chartConfig}>
      <div>{children}</div>
    </ChartContainer>,
  );
}
type TooltipEntry = Omit<
  NonNullable<ComponentProps<typeof ChartTooltipContent>["payload"]>[number],
  "graphicalItemId"
> & { series?: string };
function tooltip({
  payload = [{ name: "duration", dataKey: "duration", value: 1234, color: "red" }],
  ...props
}: Omit<Partial<ComponentProps<typeof ChartTooltipContent>>, "payload"> & {
  payload?: TooltipEntry[];
} = {}) {
  return (
    <ChartTooltipContent
      active
      payload={payload.map((item, index) => ({ graphicalItemId: String(index), ...item }))}
      {...props}
    />
  );
}

describe("chart presentation contract", () => {
  it("requires chart context for tooltip and legend", () => {
    const errors = vi.spyOn(console, "error").mockImplementation(() => undefined);
    try {
      expect(() => render(tooltip())).toThrow("useChart must be used within a <ChartContainer />");
      expect(() => render(<ChartLegendContent />)).toThrow(
        "useChart must be used within a <ChartContainer />",
      );
    } finally {
      errors.mockRestore();
    }
  });

  it("scopes light and dark series colours and omits uncoloured series", () => {
    const { container } = chart(tooltip());
    const css = container.querySelector("style")?.textContent;
    expect(css).toContain("[data-chart=chart-usage]");
    expect(css).toContain(".dark [data-chart=chart-usage]");
    expect(css).toContain("--color-duration: #123456");
    expect(css).toContain("--color-words: #ffffff");
    expect(css).toContain("--color-words: #000000");
    expect(css).not.toContain("--color-voiced");
    expect(
      screen.getByText(
        (_, element) =>
          element?.tagName === "SPAN" && element.textContent === (1234).toLocaleString(),
      ),
    ).toBeInTheDocument();
    expect(screen.getAllByText("Duration")).toHaveLength(2);
  });

  it("generates a chart identity and avoids an empty stylesheet", () => {
    const { container } = render(
      <ChartContainer config={{ plain: { label: "Plain" } }}>
        <div>Chart body</div>
      </ChartContainer>,
    );
    expect(container.querySelector("[data-chart]")?.getAttribute("data-chart")).toMatch(/^chart-/);
    expect(container.querySelector("style")).toBeNull();
  });

  it.each([false, true])("hides empty or inactive tooltips (active=%s)", (active) => {
    const { container } = chart(
      tooltip({ active, payload: active ? [] : [{ name: "duration", value: 1 }] }),
    );
    expect(container.querySelector(".shadow-xl")).toBeNull();
  });

  it("resolves a label from the label key and passes it to the formatter", () => {
    const labelFormatter = vi.fn((label) => `Total ${String(label)}`);
    chart(
      tooltip({
        labelKey: "series",
        payload: [{ name: "raw", dataKey: "raw", value: "pending", payload: { series: "words" } }],
        labelFormatter,
      }),
    );
    expect(screen.getByText("Total Words")).toBeInTheDocument();
    expect(labelFormatter).toHaveBeenCalledWith("Words", expect.any(Array));
    expect(screen.getByText("pending")).toBeInTheDocument();
  });

  it("uses a configured string label and permits an unconfigured label", () => {
    const { unmount } = chart(tooltip({ label: "words" }));
    expect(screen.getByText("Words")).toBeInTheDocument();
    unmount();
    chart(tooltip({ label: "Monday" }));
    expect(screen.getByText("Monday")).toBeInTheDocument();
  });

  it("hides the label and indicator while retaining values and series names", () => {
    const { container } = chart(tooltip({ hideLabel: true, hideIndicator: true }));
    expect(screen.getAllByText("Duration")).toHaveLength(1);
    expect(
      screen.getByText(
        (_, element) =>
          element?.tagName === "SPAN" && element.textContent === (1234).toLocaleString(),
      ),
    ).toBeInTheDocument();
    expect(container.querySelector('[style*="--color-bg"]')).toBeNull();
  });

  it.each(["line", "dashed"] as const)(
    "nests a single label beside the %s indicator and honours explicit colour",
    (indicator) => {
      const { container } = chart(tooltip({ indicator, color: "blue" }));
      const marker = container.querySelector<HTMLElement>('[style*="--color-bg"]');
      expect(marker?.style.getPropertyValue("--color-bg")).toBe("blue");
      expect(marker).toHaveClass(indicator === "line" ? "w-1" : "border-dashed");
      expect(screen.getAllByText("Duration")).toHaveLength(2);
    },
  );

  it("uses payload fill when no explicit colour is provided", () => {
    const { container } = chart(
      tooltip({ payload: [{ name: "raw", value: 2, payload: { fill: "green" } }] }),
    );
    expect(
      container
        .querySelector<HTMLElement>('[style*="--color-bg"]')
        ?.style.getPropertyValue("--color-bg"),
    ).toBe("green");
    expect(screen.getByText("raw")).toBeInTheDocument();
  });

  it("renders a configured icon, filters hidden series and omits null values", () => {
    chart(
      tooltip({
        payload: [
          { name: "voiced", value: undefined },
          { name: "hidden", type: "none", value: 8 },
        ],
      }),
    );
    expect(screen.getByLabelText("Voice series")).toBeInTheDocument();
    expect(screen.getAllByText("Voiced")).toHaveLength(2);
    expect(screen.queryByText("hidden")).not.toBeInTheDocument();
    expect(screen.queryByText("8")).not.toBeInTheDocument();
  });

  it("delegates value formatting only when the value and name are supplied", () => {
    const formatter = vi.fn((value, name) => `${String(name)}=${String(value)}`);
    chart(
      tooltip({
        formatter,
        payload: [
          { name: "duration", value: 0 },
          { name: "words", value: undefined },
          { value: 4 },
        ],
      }),
    );
    expect(screen.getByText("duration=0")).toBeInTheDocument();
    expect(formatter).toHaveBeenCalledTimes(1);
    expect(screen.getByText("4")).toBeInTheDocument();
  });

  it("resolves nameKey aliases on the item and nested payload", () => {
    chart(
      tooltip({
        nameKey: "series",
        hideLabel: true,
        payload: [
          { series: "duration", name: "first", value: 1 },
          { name: "second", payload: { series: "words" }, value: 2 },
        ],
      }),
    );
    expect(screen.getByText("Duration")).toBeInTheDocument();
    expect(screen.getByText("Words")).toBeInTheDocument();
    expect(screen.queryByText("first")).not.toBeInTheDocument();
  });

  it("renders no legend when entries are absent", () => {
    const { container } = chart(<ChartLegendContent />);
    expect(container.querySelector(".gap-4")).toBeNull();
  });

  it("renders top legend labels and icons while ignoring hidden entries", () => {
    const { container } = chart(
      <ChartLegendContent
        verticalAlign="top"
        payload={[
          { dataKey: "voiced", value: "v", color: "red" },
          { dataKey: "words", value: "w", color: "blue" },
          { dataKey: "duration", value: "d", type: "none" },
        ]}
      />,
    );
    expect(screen.getByLabelText("Voice series")).toBeInTheDocument();
    expect(screen.getByText("Words")).toBeInTheDocument();
    expect(screen.queryByText("Duration")).not.toBeInTheDocument();
    expect(container.querySelector(".pb-3")).toBeInTheDocument();
  });

  it("replaces a hidden legend icon with its colour and resolves name aliases", () => {
    const { container } = chart(
      <ChartLegendContent hideIcon nameKey="value" payload={[{ value: "voiced", color: "red" }]} />,
    );
    expect(screen.queryByLabelText("Voice series")).not.toBeInTheDocument();
    expect(screen.getByText("Voiced")).toBeInTheDocument();
    expect(container.querySelector('[style="background-color: red;"]')).toBeInTheDocument();
    expect(container.querySelector(".pt-3")).toBeInTheDocument();
  });

  it("does not emit a colour declaration for an empty theme colour", () => {
    const { container } = render(
      <ChartStyle id="empty" config={{ words: { theme: { light: "", dark: "#111" } } }} />,
    );
    expect(container.textContent).toContain("--color-words: #111");
    expect(container.textContent?.match(/--color-words:/g)).toHaveLength(1);
  });
});
