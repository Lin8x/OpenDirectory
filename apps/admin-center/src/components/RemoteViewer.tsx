// Remote desktop in the browser, three ways:
//  - Stream: the agent encodes the screen on the endpoint's GPU and the
//    browser decodes it with WebCodecs. Lowest latency, for games and
//    creative apps.
//  - RDP: Windows Remote Desktop through the server's guacd gateway.
//  - VNC: macOS Screen Sharing or any VNC server, straight to noVNC.
// Every option goes through the agent's outbound tunnel; the endpoint opens
// no inbound port.
import { useCallback, useEffect, useRef, useState } from "react";
import { Gamepad2, Keyboard, Maximize, MonitorPlay, Plug, X } from "lucide-react";
import { apiBase, isDesktop, post } from "../lib/api";
import { useQuery } from "../lib/hooks";
import type { Endpoint } from "../lib/types";
import { Badge, Button, Callout, Card, Field, Input, Select, Switch, cx } from "./ui";

type Protocol = "stream" | "rdp" | "vnc";
interface Caps { vnc: boolean; rdp: boolean; stream: boolean; rdp_note: string | null }
interface Started { session_id: string; ticket: string; ws_path: string }

const PRESETS = {
  balanced: { label: "Balanced: 1080p, 60 fps, 15 Mbit/s", fps: 60, bitrate_kbps: 15000, max_height: 1080 },
  speed: { label: "Fast network not needed: 720p, 60 fps, 6 Mbit/s", fps: 60, bitrate_kbps: 6000, max_height: 720 },
  quality: { label: "Best quality: native resolution, 60 fps, 30 Mbit/s", fps: 60, bitrate_kbps: 30000, max_height: 0 },
  smooth: { label: "Competitive games: native, 120 fps, 40 Mbit/s", fps: 120, bitrate_kbps: 40000, max_height: 0 },
  office: { label: "Office work, low bandwidth: 1080p, 30 fps, 4 Mbit/s", fps: 30, bitrate_kbps: 4000, max_height: 1080 },
} as const;
type Preset = keyof typeof PRESETS;

const wsUrl = (path: string) => `${apiBase().replace(/^http/, "ws")}${path}`;

export function RemoteViewer({ endpoint, canInput = true }: { endpoint: Endpoint; canInput?: boolean }) {
  const caps = useQuery<Caps>("/remote/capabilities");
  const [proto, setProto] = useState<Protocol>("stream");
  const [preset, setPreset] = useState<Preset>("balanced");
  const [encoder, setEncoder] = useState("auto");
  const [viewOnly, setViewOnly] = useState(!canInput);
  const [rdpUser, setRdpUser] = useState("");
  const [rdpPass, setRdpPass] = useState("");
  const [rdpDomain, setRdpDomain] = useState("");
  const [session, setSession] = useState<{ proto: Protocol; started: Started } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  if (isDesktop) return null; // The desktop app uses tunnels (RemoteDesktop card).
  if (endpoint.os === "android") return null;

  const start = async () => {
    setErr(null); setBusy(true);
    try {
      const p = PRESETS[preset];
      const started = await post<Started>(`/endpoints/${endpoint.id}/remote-desktop`, {
        protocol: proto,
        ...(proto === "stream" ? { stream: { encoder, fps: p.fps, bitrate_kbps: p.bitrate_kbps, max_height: p.max_height, input: !viewOnly } } : {}),
        ...(proto === "rdp" ? { rdp: { username: rdpUser, password: rdpPass, domain: rdpDomain } } : {}),
      });
      setRdpPass("");
      setSession({ proto, started });
    } catch (e) { setErr((e as Error).message); } finally { setBusy(false); }
  };

  if (session) {
    return <Viewer endpoint={endpoint} proto={session.proto} started={session.started} viewOnly={viewOnly} onClose={(why) => { setSession(null); if (why) setErr(why); }} />;
  }

  const c = caps.data;
  return (
    <Card title={<span className="flex items-center gap-2"><MonitorPlay className="size-4" />Remote desktop in the browser</span>}
      subtitle="See and control the screen from this browser. Nothing is opened to the network; it travels through the agent's own connection.">
      <div className="space-y-4">
        <div className="grid gap-2 sm:grid-cols-3">
          {([
            ["stream", "Low-latency stream", "GPU-encoded video for games, video and creative apps (needs FFmpeg on the endpoint)"],
            ["rdp", "Remote Desktop (RDP)", endpoint.os === "windows" ? "Full Windows Remote Desktop session" : "Needs an RDP server such as xrdp"],
            ["vnc", "VNC", endpoint.os === "macos" ? "macOS Screen Sharing" : "Any VNC server on the endpoint"],
          ] as const).map(([id, label, sub]) => {
            const off = id === "rdp" && c && !c.rdp;
            return (
              <button key={id} type="button" disabled={!!off} onClick={() => setProto(id)}
                className={cx("rounded-lg border p-3 text-left transition", proto === id ? "border-accent bg-accent-soft" : "border-line hover:bg-panel-2", off && "cursor-not-allowed opacity-50")}>
                <div className="text-sm font-medium">{label}</div>
                <div className="mt-0.5 text-xs text-ink-3">{off ? c?.rdp_note : sub}</div>
              </button>
            );
          })}
        </div>

        {proto === "stream" && (
          <div className="grid gap-3 sm:grid-cols-2">
            <Field label="Quality">
              <Select value={preset} onChange={(e) => setPreset(e.target.value as Preset)}>
                {Object.entries(PRESETS).map(([k, v]) => <option key={k} value={k}>{v.label}</option>)}
              </Select>
            </Field>
            <Field label="Video encoder" hint="Automatic picks the first GPU encoder that works, then falls back to software.">
              <Select value={encoder} onChange={(e) => setEncoder(e.target.value)}>
                <option value="auto">Automatic</option>
                <option value="nvenc">NVIDIA NVENC</option>
                <option value="amf">AMD AMF (Windows)</option>
                <option value="qsv">Intel Quick Sync</option>
                <option value="vaapi">VAAPI (Linux, AMD/Intel)</option>
                <option value="videotoolbox">Apple VideoToolbox</option>
                <option value="software">Software (x264)</option>
              </Select>
            </Field>
          </div>
        )}
        {proto === "rdp" && (
          <div className="grid gap-3 sm:grid-cols-3">
            <Field label="Username"><Input value={rdpUser} onChange={(e) => setRdpUser(e.target.value)} autoComplete="off" placeholder="e.g. the LAPS account" /></Field>
            <Field label="Password"><Input type="password" value={rdpPass} onChange={(e) => setRdpPass(e.target.value)} autoComplete="off" /></Field>
            <Field label="Domain (optional)"><Input value={rdpDomain} onChange={(e) => setRdpDomain(e.target.value)} autoComplete="off" /></Field>
          </div>
        )}
        {canInput && <Switch checked={viewOnly} onChange={setViewOnly} label="View only (don't send keyboard or mouse)" />}
        {err && <Callout tone="warn">{err}</Callout>}
        <div className="flex flex-wrap items-center gap-3">
          <Button variant="primary" icon={<Plug className="size-4" />} loading={busy} onClick={start}>Connect</Button>
          <span className="text-xs text-ink-3">{endpoint.online ? "If the endpoint asks its user for consent, the session starts once they accept." : "The endpoint is offline; the session starts when it next checks in."}</span>
        </div>
      </div>
    </Card>
  );
}

// ------------------------------------------------------------------ viewer frame

function Viewer({ endpoint, proto, started, viewOnly, onClose }: { endpoint: Endpoint; proto: Protocol; started: Started; viewOnly: boolean; onClose: (why?: string) => void }) {
  const frame = useRef<HTMLDivElement>(null);
  const [status, setStatus] = useState("Waiting for the endpoint…");
  const [info, setInfo] = useState<string | null>(null);
  const [game, setGame] = useState(false);
  const ctrlAltDel = useRef<(() => void) | null>(null);

  const fullscreen = async () => {
    const el = frame.current;
    if (!el) return;
    await el.requestFullscreen?.().catch(() => {});
    // Capture Esc, Windows key and Alt+Tab while full screen (Chrome, Edge).
    const kb = (navigator as unknown as { keyboard?: { lock?: () => Promise<void> } }).keyboard;
    await kb?.lock?.().catch(() => {});
  };

  return (
    <div ref={frame} className="overflow-hidden rounded-xl border border-line bg-black">
      <div className="flex flex-wrap items-center gap-2 border-b border-white/10 bg-[#11151c] px-3 py-2 text-sm text-white/80">
        <MonitorPlay className="size-4" /><b className="text-white">{endpoint.hostname}</b>
        <Badge tone="accent">{proto === "stream" ? "Stream" : proto.toUpperCase()}</Badge>
        <span className="truncate text-xs text-white/60">{status}{info ? ` · ${info}` : ""}</span>
        <div className="ml-auto flex items-center gap-1.5">
          {proto === "stream" && !viewOnly && (
            <button className={cx("flex h-7 items-center gap-1 rounded px-2 text-xs hover:bg-white/10", game && "bg-white/15 text-white")} title="Lock the mouse to the stream (relative movement for games). Press Esc to release."
              onClick={() => setGame((g) => !g)}><Gamepad2 className="size-3.5" />Game mode</button>
          )}
          {proto !== "stream" && !viewOnly && <button className="flex h-7 items-center gap-1 rounded px-2 text-xs hover:bg-white/10" onClick={() => ctrlAltDel.current?.()}><Keyboard className="size-3.5" />Ctrl+Alt+Del</button>}
          <button className="flex h-7 items-center gap-1 rounded px-2 text-xs hover:bg-white/10" onClick={fullscreen}><Maximize className="size-3.5" />Full screen</button>
          <button className="flex h-7 items-center gap-1 rounded px-2 text-xs hover:bg-white/10" onClick={() => onClose()}><X className="size-3.5" />Disconnect</button>
        </div>
      </div>
      <div className="relative h-[min(75vh,900px)] w-full [:fullscreen_&]:h-[calc(100vh-44px)]">
        {proto === "stream" && <StreamPlayer started={started} viewOnly={viewOnly} game={game} onGameEnd={() => setGame(false)} setStatus={setStatus} setInfo={setInfo} onClose={onClose} />}
        {proto === "vnc" && <VncPlayer started={started} viewOnly={viewOnly} setStatus={setStatus} onClose={onClose} cad={ctrlAltDel} />}
        {proto === "rdp" && <RdpPlayer started={started} viewOnly={viewOnly} setStatus={setStatus} onClose={onClose} cad={ctrlAltDel} />}
      </div>
    </div>
  );
}

interface PlayerProps { started: Started; viewOnly: boolean; setStatus: (s: string) => void; onClose: (why?: string) => void }

// ------------------------------------------------------------------ low-latency stream

/** SPS bytes after the NAL header give the avc1 codec string. */
function codecFromAu(au: Uint8Array): string | null {
  for (let i = 0; i + 7 < au.length; i++) {
    if (au[i] === 0 && au[i + 1] === 0 && au[i + 2] === 1 && (au[i + 3] & 0x1f) === 7) {
      const h = (b: number) => b.toString(16).padStart(2, "0");
      return `avc1.${h(au[i + 4])}${h(au[i + 5])}${h(au[i + 6])}`;
    }
  }
  return null;
}

function frameMsg(type: number, body: Uint8Array): Uint8Array {
  const out = new Uint8Array(body.length + 5);
  out[0] = type;
  new DataView(out.buffer).setUint32(1, body.length);
  out.set(body, 5);
  return out;
}

function StreamPlayer({ started, viewOnly, game, onGameEnd, setStatus, setInfo, onClose }: PlayerProps & { game: boolean; onGameEnd: () => void; setInfo: (s: string | null) => void }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const ws = useRef<WebSocket | null>(null);
  const enc = useRef(new TextEncoder());

  const send = useCallback((ev: object) => {
    const w = ws.current;
    if (!w || w.readyState !== 1 || viewOnly) return;
    w.send(frameMsg(10, enc.current.encode(JSON.stringify(ev))));
  }, [viewOnly]);

  useEffect(() => {
    if (!("VideoDecoder" in window)) {
      onClose("This browser can't decode the stream (no WebCodecs). Use a current Chrome, Edge, Safari or Firefox, over HTTPS.");
      return;
    }
    const cv = canvas.current!;
    const ctx = cv.getContext("2d", { alpha: false, desynchronized: true }) as CanvasRenderingContext2D;
    let decoder: VideoDecoder | null = null;
    let configured = "";
    let needKey = true;
    let buf = new Uint8Array(0);
    let frames = 0, bytes = 0, last = performance.now();
    let closedWhy: string | undefined;
    let encoderLabel = "";

    const newDecoder = () => {
      if (decoder && decoder.state !== "closed") decoder.close();
      decoder = new VideoDecoder({
        output: (f) => {
          if (cv.width !== f.displayWidth || cv.height !== f.displayHeight) { cv.width = f.displayWidth; cv.height = f.displayHeight; }
          ctx.drawImage(f, 0, 0);
          f.close();
          frames++;
        },
        error: (e) => { console.warn("decoder", e); needKey = true; configured = ""; },
      });
    };
    newDecoder();

    const onFrame = (body: Uint8Array) => {
      const key = (body[0] & 1) === 1;
      const ts = Number(new DataView(body.buffer, body.byteOffset + 1, 8).getBigUint64(0));
      const au = body.subarray(9);
      bytes += au.length;
      if (key) {
        const codec = codecFromAu(au);
        if (codec && (codec !== configured || decoder!.state !== "configured")) {
          if (decoder!.state === "closed") newDecoder();
          // The browser picks hardware decoding when it has it.
          decoder!.configure({ codec, optimizeForLatency: true });
          configured = codec;
        }
        needKey = false;
      }
      if (needKey || decoder!.state !== "configured") return;
      // Falling behind: drop to the next keyframe instead of adding delay.
      if (!key && decoder!.decodeQueueSize > 4) { needKey = true; return; }
      decoder!.decode(new EncodedVideoChunk({ type: key ? "key" : "delta", timestamp: ts, data: au }));
    };

    const sock = new WebSocket(wsUrl(started.ws_path));
    sock.binaryType = "arraybuffer";
    ws.current = sock;
    sock.onmessage = (e) => {
      if (typeof e.data === "string") return;
      const chunk = new Uint8Array(e.data as ArrayBuffer);
      const merged = new Uint8Array(buf.length + chunk.length);
      merged.set(buf); merged.set(chunk, buf.length);
      let off = 0;
      while (merged.length - off >= 5) {
        const len = new DataView(merged.buffer, off + 1, 4).getUint32(0);
        if (merged.length - off - 5 < len) break;
        const type = merged[off];
        const body = merged.subarray(off + 5, off + 5 + len);
        off += 5 + len;
        if (type === 2) onFrame(body);
        else if (type === 1) {
          const h = JSON.parse(new TextDecoder().decode(body));
          encoderLabel = `${h.encoder} · ${h.capture}`;
          setStatus(h.input ? "Connected" : `Connected, view only${h.input_note ? ` (${h.input_note})` : ""}`);
        } else if (type === 3) {
          const s = JSON.parse(new TextDecoder().decode(body));
          if (s.error) closedWhy = s.message; else setStatus(s.message);
        }
      }
      buf = merged.slice(off);
    };
    sock.onclose = () => onClose(closedWhy ?? (frames === 0 ? "The stream didn't start. The endpoint's user may have declined, or remote desktop is off for it." : undefined));
    const stats = setInterval(() => {
      const now = performance.now();
      const s = (now - last) / 1000;
      if (frames > 0) setInfo(`${encoderLabel} · ${Math.round(frames / s)} fps · ${((bytes * 8) / s / 1e6).toFixed(1)} Mbit/s · ${cv.width}×${cv.height}`);
      frames = 0; bytes = 0; last = now;
    }, 1000);
    return () => { clearInterval(stats); sock.onclose = null; sock.close(); if (decoder && decoder.state !== "closed") decoder.close(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [started.ws_path]);

  // Pointer lock for game mode.
  useEffect(() => {
    const cv = canvas.current!;
    if (game) cv.requestPointerLock?.();
    else if (document.pointerLockElement === cv) document.exitPointerLock();
    const onChange = () => { if (document.pointerLockElement !== cv && game) onGameEnd(); };
    document.addEventListener("pointerlockchange", onChange);
    return () => document.removeEventListener("pointerlockchange", onChange);
  }, [game, onGameEnd]);

  // Where on the picture a pointer event lands (the canvas is letterboxed).
  const pos = (e: React.PointerEvent | React.WheelEvent) => {
    const cv = canvas.current!;
    const r = cv.getBoundingClientRect();
    const scale = Math.min(r.width / (cv.width || 1), r.height / (cv.height || 1));
    const w = cv.width * scale, h = cv.height * scale;
    const x = (e.clientX - r.left - (r.width - w) / 2) / w;
    const y = (e.clientY - r.top - (r.height - h) / 2) / h;
    return { x: Math.min(1, Math.max(0, x)), y: Math.min(1, Math.max(0, y)) };
  };

  return (
    <canvas ref={canvas} tabIndex={0}
      className="h-full w-full cursor-default bg-black object-contain outline-none"
      onContextMenu={(e) => e.preventDefault()}
      onPointerMove={(e) => {
        if (document.pointerLockElement === canvas.current) send({ t: "rm", dx: e.movementX, dy: e.movementY });
        else send({ t: "m", ...pos(e) });
      }}
      onPointerDown={(e) => { canvas.current?.focus(); e.preventDefault(); if (document.pointerLockElement !== canvas.current) send({ t: "m", ...pos(e) }); send({ t: "b", b: e.button, d: true }); }}
      onPointerUp={(e) => { e.preventDefault(); send({ t: "b", b: e.button, d: false }); }}
      onWheel={(e) => send({ t: "w", dx: e.deltaX / 100, dy: e.deltaY / 100 })}
      onKeyDown={(e) => { e.preventDefault(); if (!e.repeat) send({ t: "k", code: e.code, d: true }); }}
      onKeyUp={(e) => { e.preventDefault(); send({ t: "k", code: e.code, d: false }); }}
      onBlur={() => send({ t: "r" })}
    />
  );
}

// ------------------------------------------------------------------ VNC (noVNC)

function VncPlayer({ started, viewOnly, setStatus, onClose, cad }: PlayerProps & { cad: React.MutableRefObject<(() => void) | null> }) {
  const host = useRef<HTMLDivElement>(null);
  const [needPw, setNeedPw] = useState(false);
  const [pw, setPw] = useState("");
  const rfbRef = useRef<{ sendCredentials: (c: { password: string; username?: string }) => void; disconnect: () => void } | null>(null);
  useEffect(() => {
    let rfb: import("@novnc/novnc").default | null = null;
    let cancelled = false;
    void import("@novnc/novnc").then(({ default: RFB }) => {
      if (cancelled) return;
      rfb = new RFB(host.current!, wsUrl(started.ws_path));
      rfb.scaleViewport = true;
      rfb.resizeSession = false;
      rfb.viewOnly = viewOnly;
      rfb.focusOnClick = true;
      rfbRef.current = rfb;
      cad.current = () => rfb?.sendCtrlAltDel();
      rfb.addEventListener("connect", () => setStatus("Connected"));
      rfb.addEventListener("credentialsrequired", () => { setNeedPw(true); setStatus("The VNC server wants a password"); });
      rfb.addEventListener("securityfailure", (e) => setStatus(`Sign-in failed: ${(e as CustomEvent).detail?.reason ?? ""}`));
      rfb.addEventListener("disconnect", (e) => onClose((e as CustomEvent).detail?.clean ? undefined : "The VNC connection closed. Is a VNC server (or Screen Sharing) running on the endpoint?"));
    });
    return () => { cancelled = true; try { rfb?.disconnect(); } catch { /* gone */ } };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [started.ws_path]);
  return (
    <>
      <div ref={host} className="h-full w-full" />
      {needPw && (
        <form className="absolute inset-x-0 top-6 mx-auto w-72 space-y-2 rounded-lg bg-panel p-4 shadow-xl" onSubmit={(e) => { e.preventDefault(); rfbRef.current?.sendCredentials({ password: pw }); setNeedPw(false); setPw(""); }}>
          <Field label="VNC password"><Input type="password" value={pw} onChange={(e) => setPw(e.target.value)} autoFocus /></Field>
          <Button type="submit" variant="primary" className="w-full">Send</Button>
        </form>
      )}
    </>
  );
}

// ------------------------------------------------------------------ RDP (Guacamole)

function RdpPlayer({ started, viewOnly, setStatus, onClose, cad }: PlayerProps & { cad: React.MutableRefObject<(() => void) | null> }) {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let client: import("guacamole-common-js").default.Client | null = null;
    let kb: import("guacamole-common-js").default.Keyboard | null = null;
    let cancelled = false;
    let ro: ResizeObserver | null = null;
    void import("guacamole-common-js").then(({ default: Guacamole }) => {
      if (cancelled) return;
      const el = host.current!;
      const tunnel = new Guacamole.WebSocketTunnel(wsUrl(started.ws_path));
      const c = new Guacamole.Client(tunnel);
      client = c;
      const display = c.getDisplay();
      const de = display.getElement();
      el.appendChild(de);
      const fit = () => {
        const w = display.getWidth(), h = display.getHeight();
        if (w && h) display.scale(Math.min(el.clientWidth / w, el.clientHeight / h));
      };
      display.onresize = fit;
      ro = new ResizeObserver(fit);
      ro.observe(el);
      const states = ["Idle", "Connecting…", "Waiting for the endpoint…", "Connected", "Disconnecting…", "Disconnected"];
      c.onstatechange = (s) => { setStatus(states[s] ?? ""); if (s === 5) onClose(); };
      c.onerror = (st) => onClose(st.message || "The Remote Desktop connection failed. Check the username and password, and that Remote Desktop is on.");
      if (!viewOnly) {
        const mouse = new Guacamole.Mouse(de);
        mouse.onEach(["mousedown", "mousemove", "mouseup"], (e) => c.sendMouseState(e.state, true));
        kb = new Guacamole.Keyboard(el);
        el.tabIndex = 0;
        kb.onkeydown = (ks) => { c.sendKeyEvent(1, ks); return false; };
        kb.onkeyup = (ks) => c.sendKeyEvent(0, ks);
        de.addEventListener("mousedown", () => el.focus());
        // Ctrl+Alt+Del keysyms: Control_L, Alt_L, Delete.
        cad.current = () => { for (const k of [0xffe3, 0xffe9, 0xffff]) c.sendKeyEvent(1, k); for (const k of [0xffff, 0xffe9, 0xffe3]) c.sendKeyEvent(0, k); };
      }
      const w = Math.round(el.clientWidth * (window.devicePixelRatio || 1));
      const h = Math.round(el.clientHeight * (window.devicePixelRatio || 1));
      c.connect(`ticket=${encodeURIComponent(started.ticket)}&width=${w}&height=${h}&dpi=${Math.round(96 * (window.devicePixelRatio || 1))}`);
    });
    return () => { cancelled = true; ro?.disconnect(); kb?.reset(); try { client?.disconnect(); } catch { /* gone */ } };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [started.ws_path]);
  return <div ref={host} className="grid h-full w-full place-items-center overflow-hidden outline-none" />;
}
