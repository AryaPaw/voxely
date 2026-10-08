import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { Activity } from "lucide-react";
import { PageHeader } from "./PageHeader";

afterEach(cleanup);

describe("PageHeader", () => {
  it("aligns the icon with the title row while keeping the description below", () => {
    const { container } = render(
      <PageHeader icon={Activity} title="History" description="Local recordings" />,
    );

    const title = screen.getByRole("heading", { name: "History" });
    const titleRow = title.parentElement;
    const iconFrame = container.querySelector("header > div > span");

    expect(titleRow).toHaveClass("flex", "min-h-7", "items-center");
    expect(title).toHaveClass("leading-7");
    expect(iconFrame).toHaveClass("size-7", "items-center", "justify-center");
    expect(screen.getByText("Local recordings")).toHaveClass("mt-1");
  });
});
