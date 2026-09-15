import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";

export async function sendSystemError(title: string, body: string): Promise<boolean> {
  try {
    await invoke("show_system_notification", { title, body });
    return true;
  } catch {
    return false;
  }
}

export function reportError(message: string, title = "Voxely"): void {
  toast.error(message);
  void sendSystemError(title, message);
}
