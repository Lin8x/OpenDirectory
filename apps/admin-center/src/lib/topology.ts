// Network Lab model and simulator. A design is a graph of network devices and
// links. `simulate` walks it the way OpenDirectory and the network would:
// agent check-in paths through routers and firewalls, DNS and DHCP reachability,
// RADIUS for Wi-Fi and VPN, domain controller replication across sites, and
// the Group Policy result each endpoint would get from its OU.
import type { OrgUnit, Policy, Setting } from "./types";

export type NodeType =
  | "dc" | "endpoint" | "server" | "switch" | "router" | "firewall" | "subnet" | "dns" | "dhcp" | "radius" | "vpn" | "ap"
  | "cloud" | "lb" | "nas" | "internet";

export interface LabNode {
  id: string; type: NodeType; name: string; x: number; y: number;
  ip?: string; cidr?: string; site?: string; os?: "windows" | "macos" | "linux"; ou?: string; vlan?: number;
  /** Firewall: allowed inbound TCP/UDP ports (e.g. "8443/tcp"). Empty means allow all. */
  allow?: string[];
  /** Endpoint gets its address from DHCP. Router relays DHCP (ip helper). */
  dhcp?: boolean;
  /** Endpoint is remote and connects through a VPN gateway. */
  remote?: boolean;
  /** Read-only map: online status. */
  online?: boolean;
  notes?: string;
}
export interface LabLink { id: string; a: string; b: string; kind: "ethernet" | "wan" | "vpn" | "wifi"; latency?: number }
export interface Design { id: string; name: string; nodes: LabNode[]; links: LabLink[]; updated_at?: string }

export const NODE_INFO: Record<NodeType, { label: string; short: string; color: string; help: string }> = {
  dc: { label: "Domain Controller", short: "DC", color: "#3a63f3", help: "An OpenDirectory Management Server. Endpoints check in to it on TCP 8443." },
  endpoint: { label: "Endpoint", short: "PC", color: "#0f9d76", help: "A managed computer running the Endpoint Agent." },
  server: { label: "Member Server", short: "SRV", color: "#5b6b82", help: "A server managed like an endpoint (file, app or database server)." },
  switch: { label: "Switch (L2)", short: "SW", color: "#64748b", help: "Connects devices in the same broadcast domain (one subnet or VLAN)." },
  router: { label: "Router (L3)", short: "RTR", color: "#b45309", help: "Routes between subnets and sites. Turn on DHCP relay to forward DHCP to another subnet." },
  firewall: { label: "Firewall", short: "FW", color: "#dc2626", help: "Filters traffic. List the ports it allows, like 8443/tcp for agent check-in." },
  subnet: { label: "Subnet / VLAN", short: "NET", color: "#0891b2", help: "An IP subnet (CIDR) and optional VLAN ID. Connect devices that live in it." },
  dns: { label: "DNS Server", short: "DNS", color: "#7c3aed", help: "Resolves names, such as the Management Server's host name." },
  dhcp: { label: "DHCP Server", short: "DHCP", color: "#9333ea", help: "Hands out IP addresses in its broadcast domain, or through a router's DHCP relay." },
  radius: { label: "RADIUS / NPS", short: "NPS", color: "#be185d", help: "Authenticates Wi-Fi (802.1X) and VPN users on UDP 1812." },
  vpn: { label: "VPN Gateway", short: "VPN", color: "#0d9488", help: "Lets remote endpoints reach the network." },
  ap: { label: "Wi-Fi Access Point", short: "AP", color: "#2563eb", help: "Wireless access. With WPA2/3-Enterprise it needs a RADIUS server." },
  cloud: { label: "Cloud Tenant", short: "IdP", color: "#0284c7", help: "Microsoft Entra ID, Google Workspace or another cloud identity provider." },
  lb: { label: "Load Balancer", short: "LB", color: "#475569", help: "Spreads connections across several servers." },
  nas: { label: "NAS / File Server", short: "NAS", color: "#a16207", help: "Network storage: SMB shares or a backup destination." },
  internet: { label: "Internet", short: "WAN", color: "#334155", help: "The public internet." },
};

export const uid = () => Math.random().toString(36).slice(2, 10);

// ------------------------------------------------------------------ IP helpers
export function parseIp(ip?: string): number | null {
  if (!ip) return null;
  const p = ip.trim().split(".").map(Number);
  if (p.length !== 4 || p.some((x) => !Number.isInteger(x) || x < 0 || x > 255)) return null;
  return ((p[0]! << 24) >>> 0) + (p[1]! << 16) + (p[2]! << 8) + p[3]!;
}
export function parseCidr(c?: string): { net: number; bits: number } | null {
  if (!c) return null;
  const [ip, b] = c.trim().split("/");
  const n = parseIp(ip); const bits = Number(b);
  if (n === null || !Number.isInteger(bits) || bits < 0 || bits > 32) return null;
  const mask = bits === 0 ? 0 : (0xffffffff << (32 - bits)) >>> 0;
  return { net: (n & mask) >>> 0, bits };
}
export function inCidr(ip: string | undefined, c: string | undefined) {
  const n = parseIp(ip); const k = parseCidr(c);
  if (n === null || !k) return false;
  const mask = k.bits === 0 ? 0 : (0xffffffff << (32 - k.bits)) >>> 0;
  return ((n & mask) >>> 0) === k.net;
}

// ------------------------------------------------------------------ simulation
export type Severity = "error" | "warning" | "info" | "ok";
export interface Issue { severity: Severity; node?: string; link?: string; title: string; detail: string }
export interface Flow { label: string; path: string[]; ok: boolean }
export interface EndpointResult { node: string; ou: string | null; settings: { kind: string; describe: string; from: string }[] }
export interface SimResult { issues: Issue[]; flows: Flow[]; gpo: EndpointResult[] }

const L3: NodeType[] = ["router", "firewall", "vpn", "internet"];

function portAllowed(fw: LabNode, port: string) {
  return !fw.allow || fw.allow.length === 0 || fw.allow.some((a) => a.trim().toLowerCase() === port || a.trim() === "*" || a.trim().toLowerCase() === "any");
}

/** Shortest path from a to b that every firewall on the way allows `port` through. */
function path(d: Design, from: string, to: string, port: string): { path: string[] | null; blockedBy: LabNode | null } {
  const adj = new Map<string, string[]>();
  for (const l of d.links) { adj.set(l.a, [...(adj.get(l.a) ?? []), l.b]); adj.set(l.b, [...(adj.get(l.b) ?? []), l.a]); }
  const byId = new Map(d.nodes.map((n) => [n.id, n]));
  const search = (respectFw: boolean) => {
    const prev = new Map<string, string>(); const seen = new Set([from]); const q = [from];
    while (q.length) {
      const cur = q.shift()!;
      if (cur === to) break;
      for (const nx of adj.get(cur) ?? []) {
        if (seen.has(nx)) continue;
        const n = byId.get(nx);
        if (respectFw && n?.type === "firewall" && nx !== to && !portAllowed(n, port)) continue;
        seen.add(nx); prev.set(nx, cur); q.push(nx);
      }
    }
    if (!seen.has(to)) return null;
    const p = [to]; while (p[0] !== from) p.unshift(prev.get(p[0]!)!);
    return p;
  };
  const ok = search(true);
  if (ok) return { path: ok, blockedBy: null };
  const any = search(false);
  const fw = any?.map((id) => byId.get(id)!).find((n) => n.type === "firewall" && !portAllowed(n, port)) ?? null;
  return { path: null, blockedBy: fw };
}

/** Nodes reachable without crossing a router/firewall (one broadcast domain). */
function broadcastDomain(d: Design, start: string) {
  const byId = new Map(d.nodes.map((n) => [n.id, n]));
  const seen = new Set([start]); const q = [start];
  while (q.length) {
    const cur = q.shift()!;
    if (cur !== start && L3.includes(byId.get(cur)!.type)) continue;
    for (const l of d.links) {
      const nx = l.a === cur ? l.b : l.b === cur ? l.a : null;
      if (nx && !seen.has(nx) && l.kind !== "wan") { seen.add(nx); q.push(nx); }
    }
  }
  return [...seen].map((id) => byId.get(id)!);
}

function effective(policies: Policy[], ous: OrgUnit[], ouId: string | null) {
  const parent = new Map(ous.map((o) => [o.id, o.parent_id]));
  const applies = (p: Policy) => {
    if (!p.enabled) return false;
    if (p.targets.all_endpoints) return true;
    let cur = ouId; let guard = 0;
    while (cur && guard++ < 64) { if (p.targets.ou_ids.includes(cur)) return true; cur = parent.get(cur) ?? null; }
    return false;
  };
  const acc = new Set(["local_accounts", "install_packages", "run_script"]);
  const out: { kind: string; describe: string; from: string }[] = []; const seen = new Set<string>();
  for (const p of policies.filter(applies).sort((a, b) => a.priority - b.priority || a.name.localeCompare(b.name))) {
    for (const s of p.settings as Setting[]) {
      if (!acc.has(s.kind)) { if (seen.has(s.kind)) continue; seen.add(s.kind); }
      out.push({ kind: s.kind, describe: describe(s), from: p.name });
    }
  }
  return out;
}

function describe(s: Setting): string {
  const v = s as Record<string, unknown>;
  const rest = Object.entries(v).filter(([k]) => k !== "kind").map(([k, x]) => `${k.replace(/_/g, " ")} ${typeof x === "object" ? JSON.stringify(x) : String(x)}`).join(", ");
  return `${s.kind.replace(/_/g, " ")}${rest ? `: ${rest}` : ""}`;
}

export function simulate(d: Design, policies: Policy[], ous: OrgUnit[]): SimResult {
  const issues: Issue[] = []; const flows: Flow[] = []; const gpo: EndpointResult[] = [];
  const by = (t: NodeType) => d.nodes.filter((n) => n.type === t);
  const name = (id: string) => d.nodes.find((n) => n.id === id)?.name ?? id;
  const dcs = by("dc");
  const clients = d.nodes.filter((n) => n.type === "endpoint" || n.type === "server");

  if (d.nodes.length === 0) return { issues: [{ severity: "info", title: "Empty design", detail: "Drag devices from the palette, connect them, then press Run." }], flows, gpo };
  if (dcs.length === 0) issues.push({ severity: "error", title: "No domain controller", detail: "Add a Domain Controller (an OpenDirectory Management Server). Endpoints have nothing to check in to." });
  if (dcs.length === 1 && clients.length > 0) issues.push({ severity: "warning", node: dcs[0]!.id, title: "Single domain controller", detail: "If it goes down, endpoints keep their last policy but can't check in. Plan regular backups; multi-server high availability is on the roadmap." });

  // Addressing.
  const ips = new Map<string, string>();
  for (const n of d.nodes) {
    if (n.ip && parseIp(n.ip) === null) issues.push({ severity: "error", node: n.id, title: `${n.name}: invalid IP address`, detail: `"${n.ip}" isn't an IPv4 address.` });
    if (n.ip && parseIp(n.ip) !== null) {
      const dup = ips.get(n.ip);
      if (dup) issues.push({ severity: "error", node: n.id, title: `Duplicate IP ${n.ip}`, detail: `${n.name} and ${name(dup)} use the same address. Only one of them will be reachable.` });
      ips.set(n.ip, n.id);
    }
    if (n.type === "subnet" && !parseCidr(n.cidr)) issues.push({ severity: "error", node: n.id, title: `${n.name}: subnet has no valid CIDR`, detail: "Enter a range such as 10.10.20.0/24." });
  }
  for (const s of by("subnet")) {
    for (const l of d.links.filter((l) => l.a === s.id || l.b === s.id)) {
      const n = d.nodes.find((x) => x.id === (l.a === s.id ? l.b : l.a))!;
      if (n.ip && !L3.includes(n.type) && n.type !== "switch" && !inCidr(n.ip, s.cidr)) {
        issues.push({ severity: "error", node: n.id, title: `${n.name} is outside ${s.name}`, detail: `${n.ip} isn't in ${s.cidr}. It won't be able to talk to its neighbors without routing.` });
      }
    }
  }

  // Agent check-in, DNS and DHCP for every endpoint.
  const dnsServers = by("dns");
  for (const c of clients) {
    if (!d.links.some((l) => l.a === c.id || l.b === c.id)) { issues.push({ severity: "error", node: c.id, title: `${c.name} isn't connected`, detail: "Connect it to a switch, access point, subnet or VPN gateway." }); continue; }
    if (dcs.length) {
      const best = dcs.map((dc) => ({ dc, r: path(d, c.id, dc.id, "8443/tcp") })).sort((a, b) => (a.r.path?.length ?? 1e9) - (b.r.path?.length ?? 1e9))[0]!;
      if (best.r.path) {
        flows.push({ label: `${c.name} checks in to ${best.dc.name} (TCP 8443)`, path: best.r.path, ok: true });
        const crossesWan = best.r.path.some((id, i) => i > 0 && d.links.some((l) => ((l.a === id && l.b === best.r.path![i - 1]) || (l.b === id && l.a === best.r.path![i - 1])) && l.kind === "wan"));
        if (crossesWan && c.site && !dcs.some((dc) => dc.site === c.site)) issues.push({ severity: "info", node: c.id, title: `${c.name} checks in across the WAN`, detail: `Site "${c.site}" has no domain controller of its own. That's fine for a few endpoints; a local server helps for many.` });
      } else {
        const fw = best.r.blockedBy;
        issues.push({ severity: "error", node: fw?.id ?? c.id, title: `${c.name} can't reach a domain controller`, detail: fw ? `${fw.name} blocks TCP 8443. Add 8443/tcp to its allowed ports.` : "There's no network path. Connect it through switches, routers or a VPN gateway." });
        flows.push({ label: `${c.name} → domain controller`, path: path(d, c.id, best.dc.id, "any").path ?? [c.id], ok: false });
      }
    }
    if (dnsServers.length) {
      const r = dnsServers.map((s) => path(d, c.id, s.id, "53/udp")).find((r) => r.path);
      if (!r) issues.push({ severity: "warning", node: c.id, title: `${c.name} can't reach DNS`, detail: "Name lookups will fail; enroll agents with the server's IP address, or allow 53/udp to a DNS server." });
    } else if (c === clients[0]) {
      issues.push({ severity: "info", title: "No DNS server in the design", detail: "Agents can use the Management Server's IP address. With DNS, give it a name like od.corp.example.com so its address can change." });
    }
    if (c.dhcp) {
      const dom = broadcastDomain(d, c.id);
      const local = dom.some((n) => n.type === "dhcp");
      const relay = dom.some((n) => n.type === "router" && n.dhcp) && by("dhcp").length > 0;
      if (!local && !relay) issues.push({ severity: "error", node: c.id, title: `${c.name} won't get an IP address`, detail: "It uses DHCP but no DHCP server is in its broadcast domain. Add one, or turn on DHCP relay on the router and add a DHCP server elsewhere." });
    }
    if (c.remote && !d.links.some((l) => (l.a === c.id || l.b === c.id) && l.kind === "vpn")) issues.push({ severity: "warning", node: c.id, title: `${c.name} is remote without a VPN link`, detail: "Connect it to a VPN gateway with a VPN link, or publish the Management Server's port 8443 to the internet behind a firewall." });
  }

  // Wi-Fi and VPN authentication.
  for (const ap of [...by("ap"), ...by("vpn")]) {
    const rs = by("radius");
    if (!rs.length) { issues.push({ severity: "warning", node: ap.id, title: `${ap.name} has no RADIUS server`, detail: "WPA2/3-Enterprise Wi-Fi and most VPNs authenticate users through RADIUS (UDP 1812). Turn on OpenDirectory's RADIUS server in Network Services, or use NPS or FreeRADIUS." }); continue; }
    const r = rs.map((s) => ({ s, r: path(d, ap.id, s.id, "1812/udp") })).find((x) => x.r.path);
    if (r) flows.push({ label: `${ap.name} authenticates users with ${r.s.name} (UDP 1812)`, path: r.r.path!, ok: true });
    else issues.push({ severity: "error", node: ap.id, title: `${ap.name} can't reach RADIUS`, detail: "Allow 1812/udp through the firewall between them." });
  }

  // Replication between domain controllers.
  for (let i = 0; i < dcs.length; i++) for (let j = i + 1; j < dcs.length; j++) {
    const r = path(d, dcs[i]!.id, dcs[j]!.id, "8443/tcp");
    if (!r.path) issues.push({ severity: "error", node: dcs[j]!.id, title: `${dcs[i]!.name} and ${dcs[j]!.name} can't replicate`, detail: "Domain controllers must reach each other. Note: multi-server replication arrives with high availability on the roadmap." });
    else {
      flows.push({ label: `${dcs[i]!.name} ↔ ${dcs[j]!.name} replication`, path: r.path, ok: true });
      const lat = d.links.filter((l) => r.path!.some((id, k) => k > 0 && ((l.a === id && l.b === r.path![k - 1]) || (l.b === id && l.a === r.path![k - 1])))).reduce((a, l) => a + (l.latency ?? (l.kind === "wan" ? 40 : 1)), 0);
      if (lat > 150) issues.push({ severity: "warning", title: `High latency between ${dcs[i]!.name} and ${dcs[j]!.name}`, detail: `About ${lat} ms. Put them in different sites so endpoints prefer their local server.` });
    }
  }
  const sites = new Set(d.nodes.map((n) => n.site).filter(Boolean));
  for (const s of sites) if (!dcs.some((dc) => dc.site === s) && clients.some((c) => c.site === s) && clients.filter((c) => c.site === s).length >= 50)
    issues.push({ severity: "warning", title: `Site "${s}" has many endpoints and no local server`, detail: "Consider a domain controller in this site." });

  // Group Policy result per endpoint, from the real directory's GPOs.
  for (const c of clients) {
    const ou = ous.find((o) => o.name.toLowerCase() === (c.ou ?? "").toLowerCase()) ?? null;
    if (c.ou && !ou) issues.push({ severity: "info", node: c.id, title: `${c.name}: OU "${c.ou}" doesn't exist yet`, detail: "The Group Policy result uses only GPOs linked to all endpoints." });
    gpo.push({ node: c.id, ou: ou?.name ?? null, settings: effective(policies, ous, ou?.id ?? null) });
  }

  if (!issues.some((i) => i.severity === "error")) issues.unshift({ severity: "ok", title: "Simulation passed", detail: `${flows.filter((f) => f.ok).length} traffic flows work. ${issues.filter((i) => i.severity === "warning").length} warning(s) to review.` });
  const order: Record<Severity, number> = { error: 0, warning: 1, ok: 2, info: 3 };
  issues.sort((a, b) => order[a.severity] - order[b.severity]);
  return { issues, flows, gpo };
}

// ------------------------------------------------------------------ templates
export function starterDesign(): Design {
  const n = (type: NodeType, name: string, x: number, y: number, extra: Partial<LabNode> = {}): LabNode => ({ id: uid(), type, name, x, y, ...extra });
  const inet = n("internet", "Internet", 80, 260);
  const fw = n("firewall", "Edge Firewall", 240, 260, { allow: ["443/tcp", "8443/tcp", "1812/udp", "53/udp"] });
  const core = n("router", "Core Router", 400, 260, { dhcp: true });
  const srv = n("subnet", "Servers VLAN 10", 560, 120, { cidr: "10.0.10.0/24", vlan: 10 });
  const usr = n("subnet", "Staff VLAN 20", 560, 400, { cidr: "10.0.20.0/24", vlan: 20 });
  const dc = n("dc", "DC01", 760, 60, { ip: "10.0.10.5", site: "HQ" });
  const dns = n("dns", "DNS01", 760, 150, { ip: "10.0.10.6", site: "HQ" });
  const dhcp = n("dhcp", "DHCP01", 760, 230, { ip: "10.0.10.7", site: "HQ" });
  const sw = n("switch", "Access Switch", 760, 400);
  const pc1 = n("endpoint", "PC-001", 920, 340, { ip: "10.0.20.21", os: "windows", ou: "Workstations", dhcp: true, site: "HQ" });
  const pc2 = n("endpoint", "MAC-002", 920, 440, { ip: "10.0.20.22", os: "macos", ou: "Workstations", dhcp: true, site: "HQ" });
  const ap = n("ap", "Wi-Fi AP", 760, 500, { ip: "10.0.20.2" });
  const L = (a: LabNode, b: LabNode, kind: LabLink["kind"] = "ethernet"): LabLink => ({ id: uid(), a: a.id, b: b.id, kind });
  return {
    id: uid(), name: "Small office",
    nodes: [inet, fw, core, srv, usr, dc, dns, dhcp, sw, pc1, pc2, ap],
    links: [L(inet, fw, "wan"), L(fw, core), L(core, srv), L(core, usr), L(srv, dc), L(srv, dns), L(srv, dhcp), L(usr, sw), L(sw, pc1), L(sw, pc2), L(sw, ap)],
  };
}

// ------------------------------------------------------------------ reference designs
type Mk = (type: NodeType, name: string, x: number, y: number, extra?: Partial<LabNode>) => LabNode;
function build(name: string, f: (n: Mk, L: (a: LabNode, b: LabNode, kind?: LabLink["kind"]) => void) => void): Design {
  const nodes: LabNode[] = []; const links: LabLink[] = [];
  const n: Mk = (type, nm, x, y, extra = {}) => { const node = { id: uid(), type, name: nm, x, y, ...extra }; nodes.push(node); return node; };
  f(n, (a, b, kind = "ethernet") => { links.push({ id: uid(), a: a.id, b: b.id, kind }); });
  return { id: uid(), name, nodes, links };
}

/** A home lab: one flat network, OpenDirectory on a mini PC or in Docker. */
export function homelabDesign(): Design {
  return build("Homelab", (n, L) => {
    const inet = n("internet", "Internet", 70, 260);
    const gw = n("firewall", "Home Router / Firewall", 230, 260, { allow: ["51820/udp"], notes: "NAT; only WireGuard is forwarded in." });
    const lan = n("subnet", "Home LAN", 420, 260, { cidr: "192.168.1.0/24" });
    const sw = n("switch", "Lab Switch", 600, 260);
    const dc = n("dc", "od-lab (mini PC)", 780, 80, { ip: "192.168.1.10", site: "Home" });
    const dns = n("dns", "DNS (OpenDirectory)", 940, 130, { ip: "192.168.1.11", site: "Home" });
    const dhcp = n("dhcp", "DHCP (OpenDirectory)", 940, 210, { ip: "192.168.1.12", site: "Home" });
    const nas = n("nas", "NAS (backups)", 780, 170, { ip: "192.168.1.20" });
    const hv = n("server", "Proxmox Host", 780, 330, { ip: "192.168.1.30", os: "linux", ou: "Servers", site: "Home" });
    const pc = n("endpoint", "Gaming PC", 940, 300, { os: "windows", ou: "Workstations", dhcp: true, site: "Home" });
    const mac = n("endpoint", "MacBook", 940, 380, { os: "macos", ou: "Workstations", dhcp: true, site: "Home" });
    const ap = n("ap", "Wi-Fi AP (WPA2-Personal)", 600, 440, { ip: "192.168.1.2" });
    const rad = n("radius", "RADIUS (OpenDirectory)", 780, 440, { ip: "192.168.1.13" });
    const vpn = n("vpn", "WireGuard", 230, 440);
    const away = n("endpoint", "Laptop (travel)", 70, 440, { os: "linux", ou: "Workstations", remote: true });
    L(inet, gw, "wan"); L(gw, lan); L(lan, sw); [dc, dns, dhcp, nas, hv, pc, mac, ap, rad].forEach((x) => L(sw, x));
    L(gw, vpn); L(vpn, lan); L(away, vpn, "vpn");
  });
}

/** A school or university campus: separate VLANs for staff, student labs and
 * Wi-Fi (802.1X, eduroam-style), routed by the core with DHCP relay. */
export function campusDesign(): Design {
  return build("Campus (K-12 / Higher Ed)", (n, L) => {
    const inet = n("internet", "Internet", 60, 280);
    const fw = n("firewall", "Edge Firewall + Web Filter", 200, 280, { allow: ["443/tcp", "51820/udp"] });
    const core = n("router", "Core Router (DHCP relay)", 360, 280, { dhcp: true });
    const srv = n("subnet", "Servers VLAN 10", 540, 70, { cidr: "10.10.10.0/24", vlan: 10 });
    const staff = n("subnet", "Staff VLAN 20", 540, 210, { cidr: "10.10.20.0/23", vlan: 20 });
    const lab = n("subnet", "Student Labs VLAN 30", 540, 350, { cidr: "10.10.30.0/23", vlan: 30 });
    const wifi = n("subnet", "Wi-Fi VLAN 40 (802.1X)", 540, 490, { cidr: "10.10.40.0/22", vlan: 40 });
    const dc1 = n("dc", "DC01", 720, 20, { ip: "10.10.10.5", site: "Main Campus" });
    const dns = n("dns", "DNS01", 860, 20, { ip: "10.10.10.6", site: "Main Campus" });
    const dhcp = n("dhcp", "DHCP01", 1000, 20, { ip: "10.10.10.7", site: "Main Campus" });
    const rad = n("radius", "RADIUS / NPS", 720, 110, { ip: "10.10.10.8", site: "Main Campus" });
    const nas = n("nas", "File Server (home drives)", 880, 110, { ip: "10.10.10.20" });
    const swS = n("switch", "Staff Switch", 720, 210);
    const t1 = n("endpoint", "STAFF-PC-01", 880, 180, { os: "windows", ou: "Staff", dhcp: true, site: "Main Campus" });
    const t2 = n("endpoint", "STAFF-MAC-02", 880, 250, { os: "macos", ou: "Staff", dhcp: true, site: "Main Campus" });
    const swL = n("switch", "Lab Switch", 720, 350);
    const l1 = n("endpoint", "LAB-01", 880, 320, { os: "windows", ou: "Student Labs", dhcp: true, site: "Main Campus" });
    const l2 = n("endpoint", "LAB-02", 880, 390, { os: "linux", ou: "Student Labs", dhcp: true, site: "Main Campus" });
    const ap = n("ap", "Campus APs", 720, 490);
    const vpn = n("vpn", "Staff VPN", 200, 450);
    const home = n("endpoint", "Teacher Laptop (home)", 60, 450, { os: "windows", ou: "Staff", remote: true });
    L(inet, fw, "wan"); L(fw, core); [srv, staff, lab, wifi].forEach((x) => L(core, x));
    [dc1, dns, dhcp, rad, nas].forEach((x) => L(srv, x));
    L(staff, swS); L(swS, t1); L(swS, t2); L(lab, swL); L(swL, l1); L(swL, l2); L(wifi, ap);
    L(fw, vpn); L(vpn, core); L(home, vpn, "vpn");
  });
}

/** An enterprise with the AD tiering model (Tier 0 identity, Tier 1 servers,
 * Tier 2 workstations), an internal firewall, a DMZ and a branch site over WAN. */
export function enterpriseDesign(): Design {
  return build("Enterprise (tiered, multi-site)", (n, L) => {
    const inet = n("internet", "Internet", 50, 300);
    const edge = n("firewall", "Edge Firewall", 180, 300, { allow: ["443/tcp", "500/udp", "4500/udp"] });
    const dmz = n("subnet", "DMZ VLAN 5", 180, 450, { cidr: "172.16.5.0/24", vlan: 5 });
    const vpn = n("vpn", "Remote Access VPN", 330, 520, { ip: "172.16.5.10" });
    const road = n("endpoint", "Sales Laptop (remote)", 60, 560, { os: "windows", ou: "Workstations", remote: true });
    const core = n("router", "HQ Core Router", 330, 300, { dhcp: true });
    const ifw = n("firewall", "Internal Firewall (tier boundary)", 490, 160, { allow: ["8443/tcp", "53/udp", "1812/udp", "67/udp"] });
    const t0 = n("subnet", "Tier 0 Identity VLAN 10", 660, 60, { cidr: "10.0.10.0/24", vlan: 10 });
    const dc1 = n("dc", "DC01", 840, 20, { ip: "10.0.10.5", site: "HQ" });
    const dc2 = n("dc", "DC02", 980, 20, { ip: "10.0.10.6", site: "HQ" });
    const dns = n("dns", "DNS01", 840, 90, { ip: "10.0.10.7", site: "HQ" });
    const rad = n("radius", "NPS01", 980, 90, { ip: "10.0.10.8", site: "HQ" });
    const dhcp = n("dhcp", "DHCP01", 1110, 55, { ip: "10.0.10.9", site: "HQ" });
    const t1 = n("subnet", "Tier 1 Servers VLAN 20", 660, 220, { cidr: "10.0.20.0/24", vlan: 20 });
    const lb = n("lb", "App Load Balancer", 840, 200, { ip: "10.0.20.5" });
    const app1 = n("server", "APP01", 980, 180, { ip: "10.0.20.11", os: "linux", ou: "Servers", site: "HQ" });
    const app2 = n("server", "APP02", 980, 250, { ip: "10.0.20.12", os: "windows", ou: "Servers", site: "HQ" });
    const fs = n("nas", "FS01", 840, 270, { ip: "10.0.20.20" });
    const t2 = n("subnet", "Tier 2 Workstations VLAN 30", 490, 380, { cidr: "10.0.30.0/22", vlan: 30 });
    const sw = n("switch", "Floor Switch", 660, 380);
    const w1 = n("endpoint", "WS-0001", 820, 350, { os: "windows", ou: "Workstations", dhcp: true, site: "HQ" });
    const w2 = n("endpoint", "WS-0002", 820, 420, { os: "macos", ou: "Workstations", dhcp: true, site: "HQ" });
    const ap = n("ap", "Corporate Wi-Fi (WPA3-Enterprise)", 660, 470, { ip: "10.0.30.2" });
    const br = n("router", "Branch Router (DHCP relay)", 980, 400, { dhcp: true });
    const bnet = n("subnet", "Branch LAN", 1110, 400, { cidr: "10.1.30.0/24" });
    const dc3 = n("dc", "DC03 (branch)", 1110, 300, { ip: "10.1.30.5", site: "Branch" });
    const bw = n("endpoint", "BR-WS-01", 1110, 500, { os: "windows", ou: "Workstations", dhcp: true, site: "Branch" });
    const cloud = n("cloud", "Microsoft Entra ID", 50, 120);
    L(inet, edge, "wan"); L(edge, dmz); L(dmz, vpn); L(road, vpn, "vpn"); L(edge, core); L(vpn, core);
    L(core, ifw); L(ifw, t0); [dc1, dc2, dns, rad, dhcp].forEach((x) => L(t0, x));
    L(ifw, t1); [lb, app1, app2, fs].forEach((x) => L(t1, x));
    L(core, t2); L(t2, sw); L(sw, w1); L(sw, w2); L(sw, ap);
    L(core, br, "wan"); L(br, bnet); L(bnet, dc3); L(bnet, bw);
    L(inet, cloud, "wan");
  });
}

/** A hardened design: default-deny segmentation, a privileged access
 * workstation (PAW) network, no direct internet for servers, and an isolated
 * backup vault. */
export function hardenedDesign(): Design {
  return build("High security (hardened, zero trust)", (n, L) => {
    const inet = n("internet", "Internet", 50, 280);
    const edge = n("firewall", "Edge Firewall (default deny)", 190, 280, { allow: ["443/tcp"] });
    const vpn = n("vpn", "ZTNA / VPN Gateway (MFA)", 190, 450);
    const remote = n("endpoint", "Remote Analyst", 50, 450, { os: "windows", ou: "Secure Workstations", remote: true });
    const core = n("router", "Core Router (DHCP relay)", 340, 280, { dhcp: true });
    const segfw = n("firewall", "Segmentation Firewall", 500, 280, { allow: ["8443/tcp", "53/udp", "1812/udp", "67/udp"] });
    const mgmt = n("subnet", "Tier 0 Management VLAN 100", 680, 80, { cidr: "10.99.0.0/24", vlan: 100 });
    const dc1 = n("dc", "DC01", 860, 30, { ip: "10.99.0.5", site: "Primary" });
    const dns = n("dns", "DNS01 (internal only)", 1000, 30, { ip: "10.99.0.6", site: "Primary" });
    const dhcp = n("dhcp", "DHCP01", 1000, 110, { ip: "10.99.0.8", site: "Primary" });
    const dc2 = n("dc", "DC02", 1140, 70, { ip: "10.99.0.9", site: "Primary" });
    const rad = n("radius", "RADIUS (802.1X)", 860, 110, { ip: "10.99.0.7", site: "Primary" });
    const paw = n("subnet", "PAW VLAN 110", 680, 230, { cidr: "10.99.10.0/24", vlan: 110 });
    const pawpc = n("endpoint", "PAW-01 (admins only)", 860, 230, { ip: "10.99.10.10", os: "windows", ou: "Privileged Access" });
    const users = n("subnet", "User VLAN 200 (802.1X)", 680, 380, { cidr: "10.20.0.0/22", vlan: 200 });
    const sw = n("switch", "Access Switch (port security)", 860, 380);
    const u1 = n("endpoint", "WS-SEC-01", 1000, 340, { os: "windows", ou: "Secure Workstations", dhcp: true, site: "Primary" });
    const u2 = n("endpoint", "WS-SEC-02", 1000, 420, { os: "linux", ou: "Secure Workstations", dhcp: true, site: "Primary" });
    const vaultfw = n("firewall", "Vault Firewall (SFTP only)", 500, 500, { allow: ["22/tcp"] });
    const vault = n("subnet", "Backup Vault VLAN 900 (offline copy)", 680, 520, { cidr: "10.250.0.0/28", vlan: 900 });
    const nas = n("nas", "Immutable Backup NAS", 860, 520, { ip: "10.250.0.5" });
    L(inet, edge, "wan"); L(edge, vpn); L(remote, vpn, "vpn"); L(vpn, core); L(edge, core);
    L(core, segfw); L(segfw, mgmt); [dc1, dc2, dns, rad, dhcp].forEach((x) => L(mgmt, x));
    L(segfw, paw); L(paw, pawpc); L(core, users); L(users, sw); L(sw, u1); L(sw, u2);
    L(segfw, vaultfw); L(vaultfw, vault); L(vault, nas);
  });
}

export const EXAMPLES: { key: string; label: string; description: string; build: () => Design }[] = [
  { key: "small-office", label: "Small office", description: "One site with server and staff VLANs.", build: starterDesign },
  { key: "homelab", label: "Homelab", description: "One flat network, OpenDirectory on a mini PC, WireGuard for remote access.", build: homelabDesign },
  { key: "campus", label: "Campus (K-12 / Higher Ed)", description: "Staff, student lab and 802.1X Wi-Fi VLANs with DHCP relay and a staff VPN.", build: campusDesign },
  { key: "enterprise", label: "Enterprise (tiered, multi-site)", description: "AD tiering model, DMZ, two HQ domain controllers, a branch DC over WAN and Entra ID.", build: enterpriseDesign },
  { key: "hardened", label: "High security (hardened)", description: "Default-deny segmentation, privileged access workstations, MFA remote access and an isolated backup vault.", build: hardenedDesign },
];
