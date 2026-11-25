// "Launch at startup" for the Admin Center: an OS login item (Windows Run key,
// macOS LaunchAgent, Linux XDG autostart) that starts the app hidden in the
// tray, so a locally hosted Management Server is back after a reboot.
import { isDesktop } from "./api";

export async function autostartEnabled(): Promise<boolean | null> {
  if (!isDesktop) return null;
  try { const m = await import("@tauri-apps/plugin-autostart"); return await m.isEnabled(); } catch { return null; }
}

export async function setAutostart(on: boolean): Promise<void> {
  if (!isDesktop) return;
  const m = await import("@tauri-apps/plugin-autostart");
  if (on) await m.enable(); else await m.disable();
}
