// Interactive terminal on an endpoint (xterm.js), relayed through the server.
import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { Plug, PlugZap } from "lucide-react";
import { openShell, type ShellConn } from "../lib/shell";
import type { Endpoint } from "../lib/types";
import { Button, Callout } from "./ui";

export function RemoteShell({ endpoint, container, autoConnect }: { endpoint: Endpoint; container?: { id: string; name: string; command: string }; autoConnect?: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);
  const conn = useRef<ShellConn | null>(null);
  const [state, setState] = useState<"idle" | "connecting" | "open" | "closed">("idle");
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => () => { conn.current?.close(); term.current?.dispose(); conn.current = null; term.current = null; }, []);
  // Deferred so React's development double-mount doesn't open two sessions.
  useEffect(() => {
    if (!autoConnect) return;
    const t = setTimeout(() => void connect(), 0);
    return () => clearTimeout(t);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const connect = async () => {
    setErr(null); setState("connecting");
    if (!term.current) {
      const t = new Terminal({ fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace", fontSize: 13, cursorBlink: true, theme: { background: "#0f1218" }, scrollback: 5000 });
      const fit = new FitAddon(); t.loadAddon(fit); t.open(host.current!); fit.fit();
      new ResizeObserver(() => { try { fit.fit(); conn.current?.resize(t.cols, t.rows); } catch { /* hidden */ } }).observe(host.current!);
      t.onData((d) => conn.current?.send(d));
      term.current = t;
    }
    const t = term.current;
    t.writeln(`\x1b[90mConnecting to ${container ? `container ${container.name} on ` : ""}${endpoint.hostname}${endpoint.online ? "" : " (offline: it connects when the endpoint next checks in)"}…\x1b[0m`);
    try {
      conn.current = await openShell(endpoint.id, t.cols, t.rows, (d) => t.write(d), () => { setState("closed"); t.writeln("\r\n\x1b[90m[session ended]\x1b[0m"); }, container && { id: container.id, command: container.command });
      setState("open"); t.focus();
    } catch (e) { setErr((e as Error).message); setState("closed"); }
  };

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        {state === "open" ? <Button icon={<PlugZap className="size-4" />} onClick={() => conn.current?.close()}>Disconnect</Button>
          : <Button variant="primary" icon={<Plug className="size-4" />} loading={state === "connecting"} onClick={connect}>{state === "closed" ? "Reconnect" : container ? "Open console" : "Open remote shell"}</Button>}
        <span className="text-xs text-ink-3">{container ? `${container.command || "bash or sh"} inside ${container.name}` : `${endpoint.os === "windows" ? "PowerShell" : "Login shell"} as ${endpoint.os === "windows" ? "SYSTEM" : "root"}`} · every session is recorded in the audit log</span>
      </div>
      {err && <Callout tone="warn">{err}</Callout>}
      <div ref={host} className="h-[460px] overflow-hidden rounded-xl border border-line bg-[#0f1218] p-2" />
    </div>
  );
}
