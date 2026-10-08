import { fixtureListen } from "./tauri-core";

export async function listen(event: string, handler: (event: { payload: unknown }) => void) {
  return fixtureListen(event, handler);
}
