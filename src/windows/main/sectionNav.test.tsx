import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { messagesFor } from "../../lib/i18n";
import { SectionNav, settingsNavItems } from "./sectionNav";

afterEach(() => cleanup());

describe("SectionNav", () => {
  it("marks the active section with aria-current", () => {
    render(
      <SectionNav
        current="filters"
        copy={messagesFor("ru")}
        localBuild
        version="0.2.13"
        onSelect={() => undefined}
      />,
    );
    expect(screen.getByRole("button", { name: /Фильтры/ })).toHaveAttribute("aria-current", "page");
    expect(screen.getByText("Voxely")).toBeInTheDocument();
    expect(screen.getByText("0.2.13 (локальная)")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /История/ })).not.toHaveAttribute("aria-current");
    expect(screen.getByRole("button", { name: /Сравнение/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Песочница/ })).toBeInTheDocument();
  });

  it("hides debug and the local suffix in a production build", () => {
    render(
      <SectionNav
        current="filters"
        copy={messagesFor("ru")}
        localBuild={false}
        version="0.2.13"
        onSelect={() => undefined}
      />,
    );
    expect(screen.getByText("Voxely")).toBeInTheDocument();
    expect(screen.getByText("0.2.13")).toBeInTheDocument();
    expect(screen.queryByText(/локальная/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Песочница/ })).not.toBeInTheDocument();
    expect(settingsNavItems(false)).not.toContain("debug");
    expect(settingsNavItems(true)).toContain("debug");
  });
});
