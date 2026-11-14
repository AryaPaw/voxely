import { useEffect, useRef, useState, type ComponentProps } from "react";
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
  onChangeRef.current = onChange;

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
      void api.setHotkeyCapture(false).then(() => onChangeRef.current(next));
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
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
