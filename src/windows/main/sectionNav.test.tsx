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
        onSelect={() => undefined}
      />,
    );
    expect(screen.getByRole("button", { name: /Замена слов/ })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.getByRole("region", { name: "Приложение" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Диктовка" })).toContainElement(
      screen.getByRole("button", { name: "Замена слов" }),
    );
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
        onSelect={() => undefined}
      />,
    );
    expect(screen.getByRole("region", { name: "Приложение" })).toBeInTheDocument();
    expect(screen.getAllByRole("button")).toHaveLength(
      new Set(screen.getAllByRole("button").map((button) => button.textContent)).size,
    );
    expect(screen.queryByText(/локальная/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Песочница/ })).not.toBeInTheDocument();
    expect(settingsNavItems(false)).not.toContain("debug");
    expect(settingsNavItems(true)).toContain("debug");
  });
});
