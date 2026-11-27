// Remote desktop through a secure tunnel: the Admin Center listens on a local
// port and carries the connection to the endpoint's own RDP, VNC or SSH
// service through the Management Server. No inbound ports on the endpoint.
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { MonitorUp, X } from "lucide-react";
import { getToken, isDesktop } from "../lib/api";
import type { Endpoint } from "../lib/types";
import { Button, Callout, Card, CopyButton } from "./ui";

const SERVICES = (os: Endpoint["os"]) => [
  ...(os === "windows" ? [{ port: 3389, label: "Remote Desktop (RDP)", client: "Remote Desktop Connection (mstsc), Microsoft Remote Desktop on Mac, or Remmina on Linux" }] : []),
  ...(os !== "windows" ? [{ port: 5900, label: os === "macos" ? "Screen Sharing (VNC)" : "VNC", client: os === "macos" ? "Screen Sharing on a Mac (vnc://), or any VNC viewer" : "any VNC viewer, such as Remmina or TigerVNC" }] : []),
  { port: 22, label: "SSH", client: "ssh, PuTTY or Windows Terminal" },
];

export function RemoteDesktop({ endpoint }: { endpoint: Endpoint }) {
  const [open, setOpen] = useState<{ port: number; local: number; label: string; client: string } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => () => { if (open) void invoke("tunnel_stop", { localPort: open.local }); }, [open]);
  if (!isDesktop) {
    return <Card title="Remote desktop with your own client"><p className="text-sm text-ink-3">To use your own RDP or VNC app through a local tunnel, use the desktop Admin Center. In a browser, use the in-browser remote desktop above.</p></Card>;
  }
  const start = async (s: { port: number; label: string; client: string }) => {
    setErr(null);
    try { const local = await invoke<number>("tunnel_start", { endpointId: endpoint.id, port: s.port, token: getToken() ?? "" }); setOpen({ ...s, local }); }
    catch (e) { setErr(String(e)); }
  };
  const addr = open ? `127.0.0.1:${open.local}` : "";
  return (
    <Card title={<span className="flex items-center gap-2"><MonitorUp className="size-4" />Remote desktop</span>} subtitle="Uses the endpoint's own service through an encrypted tunnel; nothing is exposed to the network">
      {open ? (
        <div className="space-y-3">
          <p className="text-sm">The tunnel to <b>{open.label}</b> on {endpoint.hostname} is open. Connect {open.client} to:</p>
          <div className="flex items-center gap-2"><code className="selectable rounded bg-panel-2 px-2 py-1 font-mono">{addr}</code><CopyButton text={addr} />
            {open.port === 5900 && <a className="text-sm text-accent hover:underline" href={`vnc://${addr}`}>Open in Screen Sharing</a>}
            {open.port === 22 && <code className="text-xs text-ink-3">ssh -p {open.local} user@127.0.0.1</code>}</div>
          <Button size="sm" icon={<X className="size-3.5" />} onClick={async () => { await invoke("tunnel_stop", { localPort: open.local }); setOpen(null); }}>Close tunnel</Button>
        </div>
      ) : (
        <div className="space-y-3">
          <div className="flex flex-wrap gap-2">{SERVICES(endpoint.os).map((s) => <Button key={s.port} onClick={() => start(s)}>{s.label}</Button>)}</div>
          <p className="text-xs text-ink-3">The service must be on: use the Remote Desktop or SSH server GPO settings{endpoint.os === "macos" ? ", or turn on Screen Sharing in System Settings" : ""}. Sign in with an account on the endpoint, for example its LAPS account.</p>
        </div>
      )}
      {err && <div className="mt-3"><Callout tone="warn">{err}</Callout></div>}
    </Card>
  );
}
