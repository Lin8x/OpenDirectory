// SVG canvas for network designs: pan, zoom, drag nodes, draw links, and
// show simulation results (issue badges and animated traffic flows).
import { useEffect, useRef, useState } from "react";
import { NODE_INFO, type Design, type Flow, type Issue, type LabLink, type LabNode } from "../lib/topology";

const R = 26;

export function TopologyCanvas({ design, onChange, readOnly, selected, onSelect, issues, flows, connectMode, onConnected }: {
  design: Design; onChange?: (d: Design) => void; readOnly?: boolean; selected: string | null; onSelect: (id: string | null) => void;
  issues?: Issue[]; flows?: Flow[]; connectMode?: boolean; onConnected?: () => void;
}) {
  const svg = useRef<SVGSVGElement>(null);
  const [view, setView] = useState({ x: 0, y: 0, k: 1 });
  const [drag, setDrag] = useState<{ id: string | null; dx: number; dy: number; px: number; py: number } | null>(null);
  const [linkFrom, setLinkFrom] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  const byId = new Map(design.nodes.map((n) => [n.id, n]));

  useEffect(() => { if (!connectMode) setLinkFrom(null); }, [connectMode]);
  useEffect(() => {
    if (!flows?.length) return;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduce) return;
    let raf = 0; const start = performance.now();
    const loop = (t: number) => { setTick(Math.max(0, t - start) / 1000); raf = requestAnimationFrame(loop); };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [flows]);

  // Fit the design when it's opened and whenever the canvas is resized.
  const fit = () => {
    const el = svg.current; if (!el || design.nodes.length === 0) return;
    const xs = design.nodes.map((n) => n.x); const ys = design.nodes.map((n) => n.y);
    const { width: w, height: h } = el.getBoundingClientRect(); if (!w || !h) return;
    const bw = Math.max(...xs) - Math.min(...xs) + 140; const bh = Math.max(...ys) - Math.min(...ys) + 140;
    const k = Math.min(1.3, Math.max(0.25, Math.min(w / bw, h / bh)));
    setView({ k, x: w / 2 - ((Math.min(...xs) + Math.max(...xs)) / 2) * k, y: h / 2 - ((Math.min(...ys) + Math.max(...ys)) / 2) * k });
  };
  useEffect(() => {
    const el = svg.current; if (!el) return;
    fit();
    const ro = new ResizeObserver(() => fit());
    ro.observe(el);
    return () => ro.disconnect();
  }, [design.id, readOnly ? design.nodes.length : design.nodes.length > 0]); // eslint-disable-line react-hooks/exhaustive-deps

  const toWorld = (cx: number, cy: number) => {
    const r = svg.current!.getBoundingClientRect();
    return { x: (cx - r.left - view.x) / view.k, y: (cy - r.top - view.y) / view.k };
  };
  const sev = (id: string) => {
    const mine = (issues ?? []).filter((i) => i.node === id);
    return mine.some((i) => i.severity === "error") ? "error" : mine.some((i) => i.severity === "warning") ? "warning" : null;
  };
  const clickNode = (n: LabNode) => {
    if (connectMode && !readOnly) {
      if (!linkFrom) { setLinkFrom(n.id); return; }
      if (linkFrom !== n.id && !design.links.some((l) => (l.a === linkFrom && l.b === n.id) || (l.b === linkFrom && l.a === n.id))) {
        const a = byId.get(linkFrom)!; const kind: LabLink["kind"] = a.type === "internet" || n.type === "internet" ? "wan" : a.type === "ap" || n.type === "ap" ? "wifi" : "ethernet";
        onChange?.({ ...design, links: [...design.links, { id: Math.random().toString(36).slice(2, 10), a: linkFrom, b: n.id, kind }] });
      }
      setLinkFrom(null); onConnected?.(); return;
    }
    onSelect(n.id);
  };

  return (
    <svg ref={svg} className="absolute inset-0 h-full w-full touch-none select-none" role="img" aria-label={`Network diagram: ${design.name}`}
      onWheel={(e) => { const r = svg.current!.getBoundingClientRect(); const k = Math.min(2.5, Math.max(0.25, view.k * (e.deltaY < 0 ? 1.1 : 0.9))); const mx = e.clientX - r.left; const my = e.clientY - r.top; setView({ k, x: mx - ((mx - view.x) / view.k) * k, y: my - ((my - view.y) / view.k) * k }); }}
      onPointerDown={(e) => { if (e.target === svg.current) { onSelect(null); setDrag({ id: null, dx: 0, dy: 0, px: e.clientX - view.x, py: e.clientY - view.y }); } }}
      onPointerMove={(e) => {
        if (!drag) return;
        if (drag.id === null) { setView({ ...view, x: e.clientX - drag.px, y: e.clientY - drag.py }); return; }
        if (readOnly) return;
        const p = toWorld(e.clientX, e.clientY);
        onChange?.({ ...design, nodes: design.nodes.map((n) => (n.id === drag.id ? { ...n, x: Math.round(p.x - drag.dx), y: Math.round(p.y - drag.dy) } : n)) });
      }}
      onPointerUp={() => setDrag(null)} onPointerLeave={() => setDrag(null)}>
      <defs>
        <pattern id="grid" width="24" height="24" patternUnits="userSpaceOnUse"><circle cx="1" cy="1" r="1" className="fill-line" /></pattern>
      </defs>
      <rect width="100%" height="100%" fill="url(#grid)" pointerEvents="none" />
      <g transform={`translate(${view.x},${view.y}) scale(${view.k})`}>
        {design.links.map((l) => {
          const a = byId.get(l.a); const b = byId.get(l.b); if (!a || !b) return null;
          const dash = l.kind === "wan" ? "8 6" : l.kind === "vpn" ? "2 5" : l.kind === "wifi" ? "1 4" : undefined;
          return (
            <g key={l.id} onClick={(e) => { e.stopPropagation(); onSelect(l.id); }} className="cursor-pointer">
              <line x1={a.x} y1={a.y} x2={b.x} y2={b.y} stroke="transparent" strokeWidth={14} />
              <line x1={a.x} y1={a.y} x2={b.x} y2={b.y} className={selected === l.id ? "stroke-accent" : "stroke-ink-3"} strokeWidth={selected === l.id ? 3 : 1.6} strokeDasharray={dash} strokeLinecap="round" />
              {l.kind !== "ethernet" && <text x={(a.x + b.x) / 2} y={(a.y + b.y) / 2 - 6} textAnchor="middle" className="fill-ink-3 text-[10px] uppercase">{l.kind}</text>}
            </g>
          );
        })}
        {(flows ?? []).map((f, i) => {
          const pts = f.path.map((id) => byId.get(id)).filter(Boolean) as LabNode[];
          if (pts.length < 2) return null;
          const seg = pts.length - 1; const t = ((tick * 0.6 + i * 0.37) % 1) * seg; const s = Math.max(0, Math.min(seg - 1, Math.floor(t))); const u = t - s;
          const x = pts[s]!.x + (pts[s + 1]!.x - pts[s]!.x) * u; const y = pts[s]!.y + (pts[s + 1]!.y - pts[s]!.y) * u;
          return (
            <g key={i} pointerEvents="none">
              <polyline points={pts.map((p) => `${p.x},${p.y}`).join(" ")} fill="none" stroke={f.ok ? "var(--good)" : "var(--bad)"} strokeOpacity={0.35} strokeWidth={5} strokeLinecap="round" />
              <circle cx={x} cy={y} r={5} fill={f.ok ? "var(--good)" : "var(--bad)"} />
            </g>
          );
        })}
        {design.nodes.map((n) => {
          const info = NODE_INFO[n.type]; const s = sev(n.id); const isSel = selected === n.id || linkFrom === n.id;
          return (
            <g key={n.id} transform={`translate(${n.x},${n.y})`} className="cursor-pointer"
              onPointerDown={(e) => { e.stopPropagation(); const p = toWorld(e.clientX, e.clientY); setDrag({ id: n.id, dx: p.x - n.x, dy: p.y - n.y, px: 0, py: 0 }); }}
              onClick={(e) => { e.stopPropagation(); clickNode(n); }}>
              {n.type === "subnet"
                ? <rect x={-R * 1.6} y={-R * 0.8} width={R * 3.2} height={R * 1.6} rx={10} fill={info.color} fillOpacity={0.14} stroke={info.color} strokeWidth={isSel ? 3 : 1.5} strokeDasharray="5 4" />
                : <circle r={R} fill={info.color} fillOpacity={n.online === false ? 0.35 : 1} stroke={isSel ? "var(--ink)" : "var(--panel)"} strokeWidth={isSel ? 3 : 2} />}
              <text y={4} textAnchor="middle" className={n.type === "subnet" ? "fill-ink text-[11px] font-semibold" : "fill-white text-[11px] font-bold"}>{n.type === "subnet" ? (n.cidr || "subnet") : info.short}</text>
              <text y={n.type === "subnet" ? R + 4 : R + 15} textAnchor="middle" className="fill-ink text-[12px] font-medium">{n.name}</text>
              {n.ip && <text y={n.type === "subnet" ? R + 17 : R + 28} textAnchor="middle" className="fill-ink-3 text-[10px]">{n.ip}</text>}
              {n.online !== undefined && <circle cx={R * 0.72} cy={-R * 0.72} r={5} fill={n.online ? "var(--good)" : "var(--ink-3)"} stroke="var(--panel)" strokeWidth={2} />}
              {s && <g transform={`translate(${-R * 0.75},${-R * 0.75})`}><circle r={8} fill={s === "error" ? "var(--bad)" : "var(--warn)"} /><text y={4} textAnchor="middle" className="fill-white text-[11px] font-bold">!</text></g>}
            </g>
          );
        })}
      </g>
    </svg>
  );
}
