import { useEffect, useLayoutEffect, useRef, useState, type ComponentProps } from "react";
import { api } from "../../lib/api";
import { hotkeyFromKeyboardEvent } from "../../lib/hotkey";
import { Button } from "../ui/button";

export function HotkeyCapture({
  value,
  prompt,
  onChange,
  ...buttonProps
}: {
  value: string;
  prompt: string;
  onChange: (value: string) => void;
} & Pick<ComponentProps<typeof Button>, "id" | "aria-describedby" | "aria-labelledby">) {
  const [listening, setListening] = useState(false);
  const onChangeRef = useRef(onChange);
  useLayoutEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);

  useEffect(() => {
    if (!listening) {
      return;
    }
    void api.setHotkeyCapture(true).catch(() => undefined);
    const onKey = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") {
        setListening(false);
        void api.setHotkeyCapture(false).catch(() => undefined);
        return;
      }
      const next = hotkeyFromKeyboardEvent(event);
      if (!next) {
        return;
      }
      setListening(false);
      void api
        .setHotkeyCapture(false)
        .then(() => onChangeRef.current(next))
        .catch(() => undefined);
    };
    window.addEventListener("keydown", onKey, true);
    const onBlur = () => setListening(false);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", onBlur);
      void api.setHotkeyCapture(false).catch(() => undefined);
    };
  }, [listening]);

  return (
    <Button
      {...buttonProps}
      type="button"
      variant={listening ? "default" : "outline"}
      className="w-full justify-start font-normal"
      onClick={() => setListening((current) => !current)}
    >
      {listening ? prompt : value}
    </Button>
  );
}
