import { useEffect, useState } from "react";
import { api } from "../../lib/api";
import { hotkeyFromKeyboardEvent } from "../../lib/hotkey";
import { Button } from "../ui/button";

export function HotkeyCapture({
  value,
  prompt,
  onChange,
}: {
  value: string;
  prompt: string;
  onChange: (value: string) => void;
}) {
  const [listening, setListening] = useState(false);

  useEffect(() => {
    if (!listening) {
      return;
    }
    void api.setHotkeyCapture(true);
    const onKey = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") {
        setListening(false);
        void api.setHotkeyCapture(false);
        return;
      }
      const next = hotkeyFromKeyboardEvent(event);
      if (!next) {
        return;
      }
      setListening(false);
      void api.setHotkeyCapture(false).then(() => onChange(next));
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      void api.setHotkeyCapture(false);
    };
  }, [listening, onChange]);

  return (
    <Button
      type="button"
      variant={listening ? "default" : "outline"}
      className="w-full justify-start font-normal"
      onClick={() => setListening((current) => !current)}
    >
      {listening ? prompt : value}
    </Button>
  );
}
