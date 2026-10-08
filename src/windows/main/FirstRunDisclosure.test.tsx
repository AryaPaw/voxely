import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { messagesFor } from "../../lib/i18n";
import { FirstRunDisclosure } from "./FirstRunDisclosure";

afterEach(() => cleanup());

describe("FirstRunDisclosure", () => {
  it("explains audio routing and local retention before explicit acknowledgement", async () => {
    let resolveAcknowledgement: (() => void) | undefined;
    const onAcknowledge = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveAcknowledgement = resolve;
        }),
    );
    const onOpenStorage = vi.fn();
    render(
      <FirstRunDisclosure
        copy={messagesFor("ru")}
        onAcknowledge={onAcknowledge}
        onOpenStorage={onOpenStorage}
      />,
    );

    expect(screen.getByText(/аудио в OpenRouter/i)).toBeInTheDocument();
    expect(screen.getByText(/3 дня.*1 ГБ/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Настройки хранения" }));
    expect(onOpenStorage).toHaveBeenCalledOnce();

    fireEvent.click(screen.getByRole("button", { name: "Согласен и продолжить" }));
    expect(onAcknowledge).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "Согласен и продолжить" })).toBeDisabled();
    await act(async () => resolveAcknowledgement?.());
  });
});
