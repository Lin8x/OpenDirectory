// Network Lab: design a network, press Run, and see how OpenDirectory and the
// network would behave. Also the read-only live topology of the real domain.
import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { AlertTriangle, CheckCircle2, Copy, Download, Info, Link2, Play, Plus, Save, Trash2, Upload, XCircle } from "lucide-react";
import { get, put, saveExport } from "../lib/api";
import { useQuery } from "../lib/hooks";
import { NODE_INFO, simulate, starterDesign, uid, type Design, type LabLink, type LabNode, type NodeType, type SimResult } from "../lib/topology";
import type { Endpoint, OrgUnit, Overview, Policy } from "../lib/types";
import { TopologyCanvas } from "../components/TopologyCanvas";
import { Badge, Button, Card, Explain, Field, Input, PageHeader, Select, Switch, Tabs, cx, useToast } from "../components/ui";

const PALETTE: NodeType[] = ["dc", "endpoint", "server", "switch", "router", "firewall", "subnet", "dns", "dhcp", "radius", "vpn", "ap", "cloud", "lb", "nas", "internet"];
const CLONE_KEY = "od.lab.clone";

export default function Lab() {
  const toast = useToast();
  const policies = useQuery<Policy[]>("/policies");
  const ous = useQuery<OrgUnit[]>("/ous");
  const [designs, setDesigns] = useState<Design[]>([]);
  const [cur, setCur] = useState<Design | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [connect, setConnect] = useState(false);
  const [result, setResult] = useState<SimResult | null>(null);
  const [tab, setTab] = useState<"issues" | "flows" | "gpo">("issues");
  const [dirty, setDirty] = useState(false);

  useEffect(() => {
    // Read before fetching, and remove a moment later, so React's development
    // double-run of effects sees the cloned design too.
    const cloned = sessionStorage.getItem(CLONE_KEY);
    if (cloned) setTimeout(() => sessionStorage.removeItem(CLONE_KEY), 500);
    get<Design[]>("/lab/designs").then((d) => {
      let list = d;
      if (cloned) { const c = JSON.parse(cloned) as Design; list = [c, ...d]; setDirty(true); setCur(c); }
      else setCur(d[0] ?? starterDesign());
      setDesigns(list);
    }).catch(() => setCur(starterDesign()));
  }, []);

  if (!cur) return null;
  const change = (d: Design) => { setCur(d); setDirty(true); setResult(null); };
  const node = cur.nodes.find((n) => n.id === selected);
  const link = cur.links.find((l) => l.id === selected);
  const save = async () => {
    const next = [cur, ...designs.filter((d) => d.id !== cur.id)].map((d) => (d.id === cur.id ? { ...d, updated_at: new Date().toISOString() } : d));
    try { await put("/lab/designs", next); setDesigns(next); setDirty(false); toast("good", "Design saved"); } catch (e) { toast("bad", (e as Error).message); }
  };
  const add = (t: NodeType) => {
    const n: LabNode = { id: uid(), type: t, name: `${NODE_INFO[t].short}-${cur.nodes.filter((x) => x.type === t).length + 1}`, x: 120 + Math.random() * 400, y: 100 + Math.random() * 300 };
    if (t === "subnet") n.cidr = "10.0.30.0/24";
    if (t === "firewall") n.allow = ["8443/tcp", "53/udp"];
    change({ ...cur, nodes: [...cur.nodes, n] }); setSelected(n.id);
  };
  const remove = () => {
    if (node) change({ ...cur, nodes: cur.nodes.filter((n) => n.id !== node.id), links: cur.links.filter((l) => l.a !== node.id && l.b !== node.id) });
    if (link) change({ ...cur, links: cur.links.filter((l) => l.id !== link.id) });
    setSelected(null);
  };
  const run = () => { const r = simulate(cur, policies.data ?? [], ous.data ?? []); setResult(r); setTab("issues"); };
  const setNode = (p: Partial<LabNode>) => node && change({ ...cur, nodes: cur.nodes.map((n) => (n.id === node.id ? { ...n, ...p } : n)) });
  const setLink = (p: Partial<LabLink>) => link && change({ ...cur, links: cur.links.map((l) => (l.id === link.id ? { ...l, ...p } : l)) });
  const counts = result ? { e: result.issues.filter((i) => i.severity === "error").length, w: result.issues.filter((i) => i.severity === "warning").length } : null;

  return (
    <div className="flex min-h-[calc(100vh-3rem)] flex-col">
      <PageHeader title="Network Lab" term="lab" subtitle="Design a network, press Run, and see how endpoints, Group Policy, DNS, DHCP, RADIUS and replication would behave before you build it."
        actions={<div className="flex flex-wrap gap-2">
          <Select value={cur.id} onChange={(e) => { const d = designs.find((x) => x.id === e.target.value); if (d) { setCur(d); setResult(null); setSelected(null); setDirty(false); } }} className="w-44">
            {[cur, ...designs.filter((d) => d.id !== cur.id)].map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}
          </Select>
          <Button icon={<Plus className="size-4" />} onClick={() => { const d = { id: uid(), name: "Untitled design", nodes: [], links: [] }; setCur(d); setDesigns([d, ...designs]); setResult(null); setDirty(true); }}>New</Button>
          <Button icon={<Copy className="size-4" />} onClick={() => { const d = starterDesign(); setCur(d); setDesigns([d, ...designs]); setResult(null); setDirty(true); }}>Example</Button>
          <Button icon={<Save className="size-4" />} disabled={!dirty} onClick={save}>Save</Button>
          <Button variant="primary" icon={<Play className="size-4" />} onClick={run}>Run</Button>
        </div>} />
      <div className="grid gap-4 lg:grid-cols-[180px_minmax(0,1fr)_320px]">
        <Card padded={false} className="h-[68vh] min-h-[460px] overflow-y-auto">
          <div className="border-b border-line px-3 py-2 text-xs font-semibold text-ink-3">Add a device</div>
          <div className="p-1.5">
            {PALETTE.map((t) => (
              <button key={t} title={NODE_INFO[t].help} onClick={() => add(t)} className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] hover:bg-panel-2">
                <span className="grid h-5 w-8 place-items-center rounded text-[9px] font-bold text-white" style={{ background: NODE_INFO[t].color }}>{NODE_INFO[t].short}</span>{NODE_INFO[t].label}
              </button>
            ))}
          </div>
        </Card>
        <Card padded={false} className="relative h-[68vh] min-h-[460px] overflow-hidden">
          <div className="absolute left-3 top-3 z-10 flex gap-2">
            <Button size="sm" variant={connect ? "primary" : "secondary"} icon={<Link2 className="size-3.5" />} onClick={() => setConnect(!connect)}>{connect ? "Click two devices…" : "Connect"}</Button>
            {(node || link) && <Button size="sm" icon={<Trash2 className="size-3.5" />} onClick={remove}>Delete</Button>}
            <Input value={cur.name} onChange={(e) => change({ ...cur, name: e.target.value })} className="h-8 w-44 text-xs" aria-label="Design name" />
          </div>
          {counts && <div className="absolute right-3 top-3 z-10 flex gap-1.5">{counts.e > 0 ? <Badge tone="bad">{counts.e} errors</Badge> : <Badge tone="good">No errors</Badge>}{counts.w > 0 && <Badge tone="warn">{counts.w} warnings</Badge>}</div>}
          <TopologyCanvas design={cur} onChange={change} selected={selected} onSelect={setSelected} issues={result?.issues} flows={result?.flows} connectMode={connect} onConnected={() => {}} />
        </Card>
        <div className="flex h-[68vh] min-h-[460px] flex-col gap-4 overflow-y-auto">
          {node && <NodeEditor n={node} set={setNode} ous={ous.data ?? []} />}
          {link && (
            <Card title="Link">
              <div className="space-y-3">
                <Field label="Type"><Select value={link.kind} onChange={(e) => setLink({ kind: e.target.value as LabLink["kind"] })}><option value="ethernet">Ethernet (LAN)</option><option value="wan">WAN / site link</option><option value="vpn">VPN tunnel</option><option value="wifi">Wi-Fi</option></Select></Field>
                <Field label="Latency (ms)"><Input type="number" value={link.latency ?? ""} placeholder={link.kind === "wan" ? "40" : "1"} onChange={(e) => setLink({ latency: e.target.value ? Number(e.target.value) : undefined })} /></Field>
              </div>
            </Card>
          )}
          <Card title="Results" padded={false} className="min-h-0 flex-1">
            {!result ? <p className="p-4 text-sm text-ink-3">Press <b>Run</b> to simulate the design.</p> : (
              <>
                <div className="px-3 pt-2"><Tabs value={tab} onChange={setTab} tabs={[{ id: "issues", label: `Findings (${result.issues.length})` }, { id: "flows", label: `Traffic (${result.flows.length})` }, { id: "gpo", label: "Group Policy" }]} /></div>
                <div className="max-h-[50vh] overflow-y-auto p-3">
                  {tab === "issues" && <ul className="space-y-2">{result.issues.map((i, k) => (
                    <li key={k} className={cx("cursor-pointer rounded-lg border p-2.5 text-xs", i.severity === "error" ? "border-bad/40 bg-bad/5" : i.severity === "warning" ? "border-warn/40 bg-warn-soft" : i.severity === "ok" ? "border-good/40 bg-good-soft" : "border-line")} onClick={() => i.node && setSelected(i.node)}>
                      <div className="flex items-center gap-1.5 font-semibold">{i.severity === "error" ? <XCircle className="size-3.5 text-bad" /> : i.severity === "warning" ? <AlertTriangle className="size-3.5 text-warn" /> : i.severity === "ok" ? <CheckCircle2 className="size-3.5 text-good" /> : <Info className="size-3.5 text-ink-3" />}{i.title}</div>
                      <p className="mt-1 leading-relaxed text-ink-2">{i.detail}</p>
                    </li>
                  ))}</ul>}
                  {tab === "flows" && <ul className="space-y-1.5 text-xs">{result.flows.map((f, k) => <li key={k} className="flex gap-1.5">{f.ok ? <CheckCircle2 className="mt-0.5 size-3.5 shrink-0 text-good" /> : <XCircle className="mt-0.5 size-3.5 shrink-0 text-bad" />}<span>{f.label}<span className="block text-ink-3">{f.path.map((id) => cur.nodes.find((n) => n.id === id)?.name).join(" → ")}</span></span></li>)}</ul>}
                  {tab === "gpo" && (result.gpo.length === 0 ? <p className="text-xs text-ink-3">Add endpoints to see the policy each would receive.</p> : <div className="space-y-3">{result.gpo.map((g) => (
                    <div key={g.node} className="text-xs">
                      <div className="font-semibold">{cur.nodes.find((n) => n.id === g.node)?.name} <span className="font-normal text-ink-3">· OU {g.ou ?? "none"}</span></div>
                      {g.settings.length === 0 ? <p className="text-ink-3">No GPOs apply.</p> : <ul className="mt-1 space-y-0.5">{g.settings.map((s, k) => <li key={k} className="text-ink-2">{s.describe} <span className="text-ink-3">({s.from})</span></li>)}</ul>}
                    </div>
                  ))}</div>)}
                </div>
              </>
            )}
          </Card>
          <div className="flex gap-2">
            <Button size="sm" icon={<Download className="size-3.5" />} onClick={async () => { const p = await saveExport(`${cur.name.replace(/[^\w-]+/g, "-")}.odlab.json`, JSON.stringify(cur, null, 2)); toast("good", p ? `Saved to ${p}` : "Exported"); }}>Export</Button>
            <label className="inline-flex cursor-pointer items-center gap-1.5 rounded-md border border-line px-2.5 py-1 text-xs hover:bg-panel-2"><Upload className="size-3.5" />Import
              <input type="file" accept=".json" className="hidden" onChange={async (e) => { const f = e.target.files?.[0]; if (!f) return; try { const d = JSON.parse(await f.text()) as Design; d.id = uid(); setCur(d); setDesigns([d, ...designs]); setDirty(true); setResult(null); } catch { toast("bad", "That file isn't a Network Lab design"); } }} />
            </label>
          </div>
        </div>
      </div>
    </div>
  );
}

function NodeEditor({ n, set, ous }: { n: LabNode; set: (p: Partial<LabNode>) => void; ous: OrgUnit[] }) {
  const info = NODE_INFO[n.type];
  const isClient = n.type === "endpoint" || n.type === "server";
  return (
    <Card title={info.label} subtitle={info.help}>
      <div className="space-y-3">
        <Field label="Name"><Input value={n.name} onChange={(e) => set({ name: e.target.value })} /></Field>
        {n.type === "subnet" ? <>
          <Field label="Subnet (CIDR)"><Input value={n.cidr ?? ""} placeholder="10.0.20.0/24" onChange={(e) => set({ cidr: e.target.value })} /></Field>
          <Field label="VLAN ID"><Input type="number" value={n.vlan ?? ""} onChange={(e) => set({ vlan: e.target.value ? Number(e.target.value) : undefined })} /></Field>
        </> : n.type !== "internet" && n.type !== "switch" && <Field label="IP address"><Input value={n.ip ?? ""} placeholder="10.0.20.21" onChange={(e) => set({ ip: e.target.value })} /></Field>}
        <Field label="Site" hint="Like AD Sites: where the device physically is"><Input value={n.site ?? ""} placeholder="HQ" onChange={(e) => set({ site: e.target.value })} /></Field>
        {isClient && <>
          <Field label="Operating system"><Select value={n.os ?? "windows"} onChange={(e) => set({ os: e.target.value as LabNode["os"] })}><option value="windows">Windows</option><option value="macos">macOS</option><option value="linux">Linux</option></Select></Field>
          <Field label="OU" hint="Matched by name to your real OUs for the Group Policy result">
            <Input list="lab-ous" value={n.ou ?? ""} onChange={(e) => set({ ou: e.target.value })} />
            <datalist id="lab-ous">{ous.map((o) => <option key={o.id} value={o.name} />)}</datalist>
          </Field>
          <Switch checked={!!n.dhcp} onChange={(v) => set({ dhcp: v })} label="Gets its address from DHCP" />
          <Switch checked={!!n.remote} onChange={(v) => set({ remote: v })} label="Remote (connects through VPN)" />
        </>}
        {n.type === "router" && <Switch checked={!!n.dhcp} onChange={(v) => set({ dhcp: v })} label="DHCP relay (IP helper)" />}
        {n.type === "firewall" && <Field label="Allowed ports" hint="Comma separated, e.g. 8443/tcp, 53/udp, 1812/udp. Empty allows everything.">
          <Input value={(n.allow ?? []).join(", ")} onChange={(e) => set({ allow: e.target.value.split(",").map((x) => x.trim()).filter(Boolean) })} />
        </Field>}
      </div>
    </Card>
  );
}

// ------------------------------------------------------------------ live map
export function LiveTopology() {
  const nav = useNavigate();
  const eps = useQuery<Endpoint[]>("/endpoints", { poll: 15000 });
  const ous = useQuery<OrgUnit[]>("/ous");
  const overview = useQuery<Overview>("/overview");
  const [selected, setSelected] = useState<string | null>(null);
  const design = useMemo(() => buildLive(eps.data ?? [], ous.data ?? [], overview.data), [eps.data, ous.data, overview.data]);
  const sel = design.nodes.find((n) => n.id === selected);
  return (
    <div className="flex min-h-[calc(100vh-3rem)] flex-col">
      <PageHeader title="Topology Map" term="lab" subtitle="A live, read-only view of the Management Server, the subnets your endpoints report and each endpoint's status. Clone it into the Network Lab to experiment."
        actions={<Button variant="primary" icon={<Copy className="size-4" />} onClick={() => { sessionStorage.setItem(CLONE_KEY, JSON.stringify({ ...design, id: uid(), name: `Copy of ${overview.data?.domain ?? "domain"}`, nodes: design.nodes.map((n) => ({ ...n, online: undefined })) })); nav("/lab"); }}>Clone to Lab</Button>} />
      <div className="grid min-h-0 flex-1 gap-4 lg:grid-cols-[minmax(0,1fr)_300px]">
        <Card padded={false} className="relative h-[68vh] min-h-[460px] overflow-hidden"><TopologyCanvas design={design} readOnly selected={selected} onSelect={setSelected} /></Card>
        <Card title={sel ? sel.name : "Details"}>
          {!sel ? <p className="text-sm text-ink-3">Select a device. Endpoints are grouped by the subnet of their first reported address.</p> : (
            <dl className="space-y-2 text-sm">
              <div><dt className="text-xs text-ink-3">Type</dt><dd>{NODE_INFO[sel.type].label}</dd></div>
              {sel.ip && <div><dt className="text-xs text-ink-3">Address</dt><dd className="font-mono text-xs">{sel.ip}</dd></div>}
              {sel.cidr && <div><dt className="text-xs text-ink-3">Subnet</dt><dd className="font-mono text-xs">{sel.cidr}</dd></div>}
              {sel.ou && <div><dt className="text-xs text-ink-3">OU</dt><dd>{sel.ou}</dd></div>}
              {sel.online !== undefined && <div><dt className="text-xs text-ink-3">Status</dt><dd>{sel.online ? "Online" : "Offline"}</dd></div>}
              {sel.notes && <div><dt className="text-xs text-ink-3">Notes</dt><dd>{sel.notes}</dd></div>}
              {sel.type === "endpoint" && <Button size="sm" onClick={() => nav(`/endpoints/${sel.id}`)}>Open endpoint</Button>}
            </dl>
          )}
        </Card>
      </div>
      <div className="mt-3"><Explain>Subnets are worked out from the addresses endpoints report, assuming /24 networks. Switches, routers and firewalls aren't discovered automatically.</Explain></div>
    </div>
  );
}

function buildLive(eps: Endpoint[], ous: OrgUnit[], ov: Overview | null): Design {
  const host = ov?.server.endpoint_url ? new URL(ov.server.endpoint_url).hostname : "";
  const dc: LabNode = { id: "dc", type: "dc", name: ov?.domain ?? "Management Server", x: 0, y: 0, ip: host, notes: `Version ${ov?.server.version ?? ""}`, online: true };
  const nets = new Map<string, Endpoint[]>();
  for (const e of eps) {
    const ip = e.inventory.interfaces.flatMap((i) => i.ips).find((x) => /^\d+\.\d+\.\d+\.\d+$/.test(x) && !x.startsWith("127.") && !x.startsWith("169.254.")) ?? "";
    const key = ip ? ip.split(".").slice(0, 3).join(".") + ".0/24" : "unknown";
    nets.set(key, [...(nets.get(key) ?? []), e]);
  }
  const nodes: LabNode[] = [dc]; const links: LabLink[] = [];
  const keys = [...nets.keys()].sort();
  keys.forEach((k, i) => {
    const sx = (i - (keys.length - 1) / 2) * 260;
    const subnet: LabNode = { id: `net-${k}`, type: "subnet", name: k === "unknown" ? "Unknown network" : `Subnet ${k.replace(".0/24", ".x")}`, cidr: k === "unknown" ? undefined : k, x: sx, y: 160 };
    nodes.push(subnet); links.push({ id: `l-${k}`, a: dc.id, b: subnet.id, kind: "ethernet" });
    nets.get(k)!.forEach((e, j) => {
      const ip = e.inventory.interfaces.flatMap((x) => x.ips).find((x) => x.startsWith(k.split(".").slice(0, 3).join(".")));
      nodes.push({ id: e.id, type: "endpoint", name: e.hostname, x: sx + ((j % 3) - 1) * 80, y: 290 + Math.floor(j / 3) * 90, ip, os: e.os, ou: ous.find((o) => o.id === e.ou_id)?.name, online: e.online });
      links.push({ id: `l-${e.id}`, a: subnet.id, b: e.id, kind: "ethernet" });
    });
  });
  return { id: "live", name: "Live topology", nodes, links };
}
