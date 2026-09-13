import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { toast } from "sonner";

export async function sendSystemError(title: string, body: string): Promise<boolean> {
  try {
    let granted = await isPermissionGranted();
    if (!granted) {
      granted = (await requestPermission()) === "granted";
    }
    if (!granted) {
      return false;
    }
    sendNotification({ title, body });
    return true;
  } catch {
    return false;
  }
}

export function reportError(message: string, title = "Voxely"): void {
  toast.error(message);
  void sendSystemError(title, message);
}
