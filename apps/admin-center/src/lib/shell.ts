// Remote shell transport. In the desktop app the Rust side holds the
// WebSocket (it pins the server's certificate); in a browser the page opens it.
import { invoke } from "@tauri-apps/api/core";
import { apiBase, isDesktop, post } from "./api";

export interface ShellConn { send: (data: string) => void; resize: (cols: number, rows: number) => void; close: () => void }

const enc = new TextEncoder();
const b64 = (s: string) => { const b = enc.encode(s); let x = ""; for (const c of b) x += String.fromCharCode(c); return btoa(x); };
const unb64 = (s: string) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0));

export async function openShell(endpointId: string, cols: number, rows: number, onData: (d: Uint8Array) => void, onClose: () => void): Promise<ShellConn> {
  const { session_id, ticket } = await post<{ session_id: string; ticket: string }>(`/endpoints/${endpointId}/shell`, { cols, rows });
  if (isDesktop) {
    const { listen } = await import("@tauri-apps/api/event");
    const un1 = await listen<{ id: string; data: string }>("shell://data", (e) => { if (e.payload.id === session_id) onData(unb64(e.payload.data)); });
    const un2 = await listen<string>("shell://closed", (e) => { if (e.payload === session_id) { un1(); un2(); onClose(); } });
    await invoke("shell_connect", { sessionId: session_id, ticket });
    return {
      send: (d) => void invoke("shell_send", { sessionId: session_id, data: b64(d) }).catch(() => {}),
      resize: (c, r) => void invoke("shell_send", { sessionId: session_id, resize: [c, r] }).catch(() => {}),
      close: () => void invoke("shell_send", { sessionId: session_id, close: true }).catch(() => {}),
    };
  }
  const ws = new WebSocket(`${apiBase().replace(/^http/, "ws")}/api/v1/shell/${session_id}/ws?ticket=${ticket}`);
  ws.binaryType = "arraybuffer";
  ws.onmessage = (e) => onData(typeof e.data === "string" ? enc.encode(e.data) : new Uint8Array(e.data as ArrayBuffer));
  ws.onclose = onClose;
  await new Promise<void>((res, rej) => { ws.onopen = () => res(); ws.onerror = () => rej(new Error("Couldn't open the shell connection")); });
  return {
    send: (d) => ws.readyState === 1 && ws.send(enc.encode(d)),
    resize: (c, r) => ws.readyState === 1 && ws.send(JSON.stringify({ resize: [c, r] })),
    close: () => ws.close(),
  };
}
