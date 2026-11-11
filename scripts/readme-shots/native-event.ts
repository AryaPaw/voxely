import { listen as nativeListen } from "../../node_modules/@tauri-apps/api/event";
import { listen as fixtureListen } from "./tauri-event";

export function listen(event: string, handler: (event: { payload: unknown }) => void) {
  return event.startsWith("tauri://")
    ? nativeListen(event, handler)
    : fixtureListen(event, handler);
}
