// Native window controls and generated cue samples cross IPC. User data stays in the fixture.
import { invoke as nativeInvoke, isTauri } from "../../node_modules/@tauri-apps/api/core";
import { invoke as fixtureInvoke, convertFileSrc } from "./tauri-core";

export { convertFileSrc, isTauri };

export function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (
    command === "get_cue_previews" ||
    command.startsWith("plugin:window|") ||
    command.startsWith("plugin:event|")
  ) {
    return nativeInvoke<T>(command, args);
  }
  return fixtureInvoke<T>(command, args);
}
