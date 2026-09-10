import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { messagesFor } from "../../lib/i18n";
import { SectionNav } from "./sectionNav";

afterEach(() => cleanup());

describe("SectionNav", () => {
  it("marks the active section with aria-current", () => {
    render(<SectionNav current="filters" copy={messagesFor("ru")} onSelect={() => undefined} />);
    expect(screen.getByRole("button", { name: /Фильтры/ })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: /Записи/ })).not.toHaveAttribute("aria-current");
  });
});
