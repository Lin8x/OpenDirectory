// Network Services hosted by the Management Server: DNS, DHCP, a certificate
// authority and RADIUS.
import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { AlertTriangle, CheckCircle2, FileKey2, Globe, Network, Plus, Radio, Trash2, XCircle } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { apiBase, post, put } from "../lib/api";
import { dateTime } from "../lib/format";
import { Badge, Button, Callout, Card, CopyButton, Dialog, Explain, Field, Input, Loading, PageHeader, Select, Switch, Tabs, Textarea, useToast } from "../components/ui";

const KEEP = "••••••••";
interface Status { running: boolean; error: string }
type Tab = "dns" | "dhcp" | "ca" | "radius";

export default function NetworkServices() {
  const [params, setParams] = useSearchParams();
  const tab = (params.get("tab") as Tab) || "dns";
  return (
    <>
      <PageHeader title="Network Services" term="netsvc" subtitle="Services the Management Server can host for your network, like the roles of a Windows Server: DNS, DHCP, a certificate authority and RADIUS." />
      <div className="mb-4"><Tabs value={tab} onChange={(t) => setParams({ tab: t })} tabs={[
        { id: "dns", label: <span className="flex items-center gap-1.5"><Globe className="size-3.5" />DNS</span> },
        { id: "dhcp", label: <span className="flex items-center gap-1.5"><Network className="size-3.5" />DHCP</span> },
        { id: "ca", label: <span className="flex items-center gap-1.5"><FileKey2 className="size-3.5" />Certificate Authority</span> },
        { id: "radius", label: <span className="flex items-center gap-1.5"><Radio className="size-3.5" />RADIUS</span> },
      ]} /></div>
      {tab === "dns" && <Dns />}
      {tab === "dhcp" && <Dhcp />}
      {tab === "ca" && <Ca />}
      {tab === "radius" && <Radius />}
      <div className="mt-5"><Explain>DNS (53), DHCP (67) and RADIUS (1812) use privileged or well-known ports: run the Management Server as a service (root/SYSTEM) or change the listen address. In Docker, publish the ports and add the NET_BIND_SERVICE capability.</Explain></div>
    </>
  );
}

function StatusBadge({ s, enabled }: { s?: Status; enabled: boolean }) {
  if (!enabled) return <Badge>Off</Badge>;
  if (s?.error) return <Badge tone="bad"><span className="flex items-center gap-1"><XCircle className="size-3" />Stopped</span></Badge>;
  return <Badge tone="good"><span className="flex items-center gap-1"><CheckCircle2 className="size-3" />Running</span></Badge>;
}

// ------------------------------------------------------------------ DNS
interface DnsRecord { id: string; name: string; type: string; value: string; ttl?: number | null; priority: number; weight: number; port: number }
interface DnsView { config: { enabled: boolean; listen: string; forwarders: string[]; dynamic_updates: boolean; default_ttl: number }; status: Status; zone: string | null; records: DnsRecord[]; generated: DnsRecord[] }
const NIL = "00000000-0000-0000-0000-000000000000";

function Dns() {
  const toast = useToast();
  const v = useQuery<DnsView>("/dns");
  const [cfg, setCfg] = useState<DnsView["config"] | null>(null);
  const [recs, setRecs] = useState<DnsRecord[]>([]);
  useEffect(() => { if (v.data) { setCfg(v.data.config); setRecs(v.data.records); } }, [v.data]);
  if (!v.data || !cfg) return <Loading />;
  const saveCfg = async () => { try { const st = await put<Status>("/dns/config", cfg); toast(st.error ? "bad" : "good", st.error || "DNS settings saved"); void v.reload(); } catch (e) { toast("bad", (e as Error).message); } };
  const saveRecs = async () => { try { setRecs(await put<DnsRecord[]>("/dns/records", recs)); toast("good", "Records saved"); void v.reload(); } catch (e) { toast("bad", (e as Error).message); } };
  return (
    <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_360px]">
      <Card title={`Zone: ${v.data.zone ?? ""}`} subtitle="Records you add here, plus the ones generated below" actions={<Button size="sm" icon={<Plus className="size-3.5" />} onClick={() => setRecs([...recs, { id: NIL, name: "", type: "A", value: "", priority: 10, weight: 0, port: 0 }])}>Add record</Button>}>
        <div className="space-y-2">
          {recs.length === 0 && <p className="text-sm text-ink-3">No records yet.</p>}
          {recs.map((r, i) => (
            <div key={i} className="grid items-center gap-2 sm:grid-cols-[1fr_90px_1.4fr_70px_auto]">
              <Input value={r.name} placeholder="www or @" onChange={(e) => setRecs(recs.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))} />
              <Select value={r.type} onChange={(e) => setRecs(recs.map((x, j) => (j === i ? { ...x, type: e.target.value } : x)))}>{["A", "AAAA", "CNAME", "MX", "SRV", "TXT", "PTR", "NS"].map((t) => <option key={t}>{t}</option>)}</Select>
              <Input value={r.value} placeholder={r.type === "A" ? "10.0.0.5" : r.type === "TXT" ? "text" : "target"} onChange={(e) => setRecs(recs.map((x, j) => (j === i ? { ...x, value: e.target.value } : x)))} />
              {r.type === "SRV" ? <Input type="number" value={r.port} title="Port" onChange={(e) => setRecs(recs.map((x, j) => (j === i ? { ...x, port: Number(e.target.value) } : x)))} />
                : r.type === "MX" ? <Input type="number" value={r.priority} title="Preference" onChange={(e) => setRecs(recs.map((x, j) => (j === i ? { ...x, priority: Number(e.target.value) } : x)))} /> : <span />}
              <button className="text-ink-3 hover:text-bad" onClick={() => setRecs(recs.filter((_, j) => j !== i))} aria-label="Remove record"><Trash2 className="size-4" /></button>
            </div>
          ))}
          <div className="pt-2"><Button variant="primary" onClick={saveRecs}>Save records</Button></div>
          <h4 className="pt-4 text-sm font-semibold">Generated automatically</h4>
          <div className="max-h-56 overflow-y-auto rounded-lg border border-line text-xs">
            {v.data.generated.map((r, i) => <div key={i} className="flex gap-3 border-b border-line px-3 py-1.5 last:border-0"><span className="w-40 truncate font-mono">{r.name}</span><Badge className="py-0">{r.type}</Badge><span className="font-mono text-ink-2">{r.value}{r.port ? `:${r.port}` : ""}</span></div>)}
          </div>
        </div>
      </Card>
      <Card title="DNS server" actions={<StatusBadge s={v.data.status} enabled={v.data.config.enabled} />}>
        <div className="space-y-3">
          {v.data.status.error && <Callout tone="warn">{v.data.status.error}</Callout>}
          <Switch checked={cfg.enabled} onChange={(x) => setCfg({ ...cfg, enabled: x })} label="Answer DNS queries" />
          <Field label="Listen on"><Input value={cfg.listen} onChange={(e) => setCfg({ ...cfg, listen: e.target.value })} /></Field>
          <Field label="Forwarders" hint="Comma separated. Names outside the zone are sent here."><Input value={cfg.forwarders.join(", ")} onChange={(e) => setCfg({ ...cfg, forwarders: e.target.value.split(",").map((x) => x.trim()).filter(Boolean) })} /></Field>
          <Switch checked={cfg.dynamic_updates} onChange={(x) => setCfg({ ...cfg, dynamic_updates: x })} label="Register endpoints and DHCP clients automatically" />
          <Button variant="primary" onClick={saveCfg}>Save and apply</Button>
          <p className="text-xs text-ink-3">Point your DHCP server's DNS option (006) at this server so endpoints use it.</p>
        </div>
      </Card>
    </div>
  );
}

// ------------------------------------------------------------------ DHCP
interface Scope { name: string; network: string; range_start: string; range_end: string; gateway: string; dns: string[]; domain: string; lease_hours: number; reservations: { mac: string; ip: string; name: string }[] }
interface DhcpView { config: { enabled: boolean; listen: string; server_ip: string; scopes: Scope[] }; status: Status; leases: { mac: string; ip: string; hostname: string; expires: string; scope: string }[]; suggested_server_ip: string }

function Dhcp() {
  const toast = useToast();
  const v = useQuery<DhcpView>("/dhcp", { poll: 15000 });
  const [cfg, setCfg] = useState<DhcpView["config"] | null>(null);
  useEffect(() => { if (v.data && !cfg) setCfg({ ...v.data.config, server_ip: v.data.config.server_ip || v.data.suggested_server_ip }); }, [v.data]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!v.data || !cfg) return <Loading />;
  const setScope = (i: number, p: Partial<Scope>) => setCfg({ ...cfg, scopes: cfg.scopes.map((s, j) => (j === i ? { ...s, ...p } : s)) });
  return (
    <div className="space-y-5">
      <Callout tone="warn" title="Only one DHCP server per network">Turn this on only where no other DHCP server (often your router) hands out addresses, or two servers will give clients conflicting answers.</Callout>
      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_360px]">
        <Card title="Scopes" actions={<Button size="sm" icon={<Plus className="size-3.5" />} onClick={() => setCfg({ ...cfg, scopes: [...cfg.scopes, { name: `Scope ${cfg.scopes.length + 1}`, network: "10.0.20.0/24", range_start: "10.0.20.100", range_end: "10.0.20.200", gateway: "10.0.20.1", dns: [], domain: "", lease_hours: 8, reservations: [] }] })}>Add scope</Button>}>
          <div className="space-y-4">
            {cfg.scopes.length === 0 && <p className="text-sm text-ink-3">No scopes yet.</p>}
            {cfg.scopes.map((s, i) => (
              <div key={i} className="rounded-xl border border-line p-3">
                <div className="grid gap-2 sm:grid-cols-3">
                  <Field label="Name"><Input value={s.name} onChange={(e) => setScope(i, { name: e.target.value })} /></Field>
                  <Field label="Network"><Input value={s.network} onChange={(e) => setScope(i, { network: e.target.value })} /></Field>
                  <Field label="Lease (hours)"><Input type="number" value={s.lease_hours} onChange={(e) => setScope(i, { lease_hours: Number(e.target.value) })} /></Field>
                  <Field label="Range start"><Input value={s.range_start} onChange={(e) => setScope(i, { range_start: e.target.value })} /></Field>
                  <Field label="Range end"><Input value={s.range_end} onChange={(e) => setScope(i, { range_end: e.target.value })} /></Field>
                  <Field label="Gateway (003)"><Input value={s.gateway} onChange={(e) => setScope(i, { gateway: e.target.value })} /></Field>
                  <Field label="DNS servers (006)"><Input value={s.dns.join(", ")} onChange={(e) => setScope(i, { dns: e.target.value.split(",").map((x) => x.trim()).filter(Boolean) })} /></Field>
                  <Field label="DNS domain (015)"><Input value={s.domain} onChange={(e) => setScope(i, { domain: e.target.value })} /></Field>
                </div>
                <div className="mt-2 text-xs font-semibold">Reservations</div>
                {s.reservations.map((r, k) => (
                  <div key={k} className="mt-1 grid grid-cols-[1fr_1fr_1fr_auto] gap-2">
                    <Input value={r.mac} placeholder="aa:bb:cc:dd:ee:ff" onChange={(e) => setScope(i, { reservations: s.reservations.map((x, n) => (n === k ? { ...x, mac: e.target.value } : x)) })} />
                    <Input value={r.ip} placeholder="10.0.20.50" onChange={(e) => setScope(i, { reservations: s.reservations.map((x, n) => (n === k ? { ...x, ip: e.target.value } : x)) })} />
                    <Input value={r.name} placeholder="printer" onChange={(e) => setScope(i, { reservations: s.reservations.map((x, n) => (n === k ? { ...x, name: e.target.value } : x)) })} />
                    <button className="text-ink-3 hover:text-bad" onClick={() => setScope(i, { reservations: s.reservations.filter((_, n) => n !== k) })} aria-label="Remove"><Trash2 className="size-4" /></button>
                  </div>
                ))}
                <div className="mt-2 flex justify-between">
                  <Button size="sm" onClick={() => setScope(i, { reservations: [...s.reservations, { mac: "", ip: "", name: "" }] })}>Add reservation</Button>
                  <Button size="sm" variant="ghost" icon={<Trash2 className="size-3.5" />} onClick={() => setCfg({ ...cfg, scopes: cfg.scopes.filter((_, j) => j !== i) })}>Remove scope</Button>
                </div>
              </div>
            ))}
          </div>
        </Card>
        <Card title="DHCP server" actions={<StatusBadge s={v.data.status} enabled={v.data.config.enabled} />}>
          <div className="space-y-3">
            {v.data.status.error && <Callout tone="warn">{v.data.status.error}</Callout>}
            <Switch checked={cfg.enabled} onChange={(x) => setCfg({ ...cfg, enabled: x })} label="Hand out addresses" />
            <Field label="This server's IP on that network"><Input value={cfg.server_ip} onChange={(e) => setCfg({ ...cfg, server_ip: e.target.value })} /></Field>
            <Field label="Listen on"><Input value={cfg.listen} onChange={(e) => setCfg({ ...cfg, listen: e.target.value })} /></Field>
            <Button variant="primary" onClick={async () => { try { const st = await put<Status>("/dhcp", cfg); toast(st.error ? "bad" : "good", st.error || "DHCP settings saved"); void v.reload(); } catch (e) { toast("bad", (e as Error).message); } }}>Save and apply</Button>
            <h4 className="pt-2 text-sm font-semibold">Leases ({v.data.leases.length})</h4>
            <div className="max-h-56 overflow-y-auto text-xs">{v.data.leases.map((l) => <div key={l.mac} className="flex justify-between border-b border-line py-1"><span className="font-mono">{l.ip}</span><span>{l.hostname || l.mac}</span><span className="text-ink-3">until {dateTime(l.expires)}</span></div>)}</div>
          </div>
        </Card>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ CA
interface Issued { id: string; serial: string; common_name: string; sans: string[]; kind: string; not_after: string; issued_at: string; issued_by: string; revoked_at: string | null }
interface CaView { initialized: boolean; common_name: string | null; not_after: string | null; root_pem: string | null; issued: Issued[] }

function Ca() {
  const toast = useToast();
  const v = useQuery<CaView>("/ca");
  const [issue, setIssue] = useState(false);
  if (!v.data) return <Loading />;
  if (!v.data.initialized) return (
    <Card title="Certificate Authority">
      <p className="mb-3 max-w-2xl text-sm text-ink-2">Create a root certificate authority for your domain. Use it for internal websites, Wi-Fi and VPN servers, and device certificates. Endpoints trust it through the <b>Trusted root certificate</b> GPO setting.</p>
      <Button variant="primary" onClick={async () => { try { await post("/ca", {}); toast("good", "Certificate authority created"); void v.reload(); } catch (e) { toast("bad", (e as Error).message); } }}>Create certificate authority</Button>
    </Card>
  );
  const base = apiBase();
  return (
    <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_360px]">
      <Card title="Issued certificates" padded={false} actions={<Button size="sm" variant="primary" icon={<Plus className="size-3.5" />} onClick={() => setIssue(true)}>Issue certificate</Button>}>
        {v.data.issued.length === 0 ? <p className="p-4 text-sm text-ink-3">None yet.</p> : (
          <ul className="divide-y divide-line">
            {v.data.issued.map((c) => (
              <li key={c.id} className="flex items-center gap-3 px-4 py-2.5 text-sm">
                <div className="min-w-0 flex-1"><div className="flex items-center gap-2"><span className="font-medium">{c.common_name}</span><Badge className="py-0">{c.kind}</Badge>{c.revoked_at && <Badge tone="bad" className="py-0">Revoked</Badge>}</div>
                  <div className="truncate text-xs text-ink-3">serial {c.serial.slice(0, 16)}… · expires {dateTime(c.not_after)}</div></div>
                {!c.revoked_at && <Button size="sm" variant="ghost" onClick={async () => { await post(`/ca/certificates/${c.id}/revoke`); toast("good", "Revoked; the CRL updates right away"); void v.reload(); }}>Revoke</Button>}
              </li>
            ))}
          </ul>
        )}
      </Card>
      <Card title={v.data.common_name ?? "Root CA"} subtitle={`Valid until ${dateTime(v.data.not_after)}`}>
        <div className="space-y-3 text-sm">
          <div className="flex gap-2"><CopyButton text={v.data.root_pem ?? ""} label="Copy root certificate" /></div>
          <p className="text-xs text-ink-3">Root: <code className="font-mono">{base}/api/v1/ca/root.pem</code><br />CRL: <code className="font-mono">{base}/api/v1/ca/crl.pem</code></p>
          <p className="text-xs text-ink-3">To make endpoints trust it, add the <b>Trusted root certificate</b> setting to a GPO and paste the root certificate.</p>
        </div>
      </Card>
      {issue && <IssueDialog onClose={() => setIssue(false)} onDone={() => { setIssue(false); void v.reload(); }} />}
    </div>
  );
}

function IssueDialog({ onClose, onDone }: { onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const [f, setF] = useState({ kind: "server", common_name: "", sans: "", days: 397, csr_pem: "" });
  const [out, setOut] = useState<{ chain_pem: string; private_key_pem: string | null } | null>(null);
  return (
    <Dialog open onClose={out ? onDone : onClose} wide title="Issue a certificate"
      footer={out ? <Button variant="primary" onClick={onDone}>Done</Button> : <><Button onClick={onClose}>Cancel</Button><Button variant="primary" onClick={async () => {
        try { setOut(await post("/ca/issue", { ...f, sans: f.sans.split(",").map((x) => x.trim()).filter(Boolean) })); } catch (e) { toast("bad", (e as Error).message); }
      }}>Issue</Button></>}>
      {out ? (
        <div className="space-y-3">
          <Field label="Certificate chain (PEM)"><Textarea rows={6} readOnly className="font-mono text-[11px]" value={out.chain_pem} /></Field>
          <CopyButton text={out.chain_pem} label="Copy chain" />
          {out.private_key_pem && <>
            <Callout tone="warn" title="Save the private key now">It isn't stored on the server and can't be shown again.</Callout>
            <Field label="Private key (PEM)"><Textarea rows={6} readOnly className="font-mono text-[11px]" value={out.private_key_pem} /></Field>
            <CopyButton text={out.private_key_pem} label="Copy private key" />
          </>}
        </div>
      ) : (
        <div className="space-y-3">
          <div className="grid gap-3 sm:grid-cols-3">
            <Field label="Type"><Select value={f.kind} onChange={(e) => setF({ ...f, kind: e.target.value })}><option value="server">Server (TLS)</option><option value="client">Client / user</option></Select></Field>
            <Field label="Common name"><Input value={f.common_name} placeholder="intranet.corp.example.com" onChange={(e) => setF({ ...f, common_name: e.target.value })} /></Field>
            <Field label="Valid for (days)"><Input type="number" value={f.days} onChange={(e) => setF({ ...f, days: Number(e.target.value) })} /></Field>
          </div>
          <Field label="Other names (SANs)" hint="Comma separated host names or IP addresses"><Input value={f.sans} onChange={(e) => setF({ ...f, sans: e.target.value })} /></Field>
          <Field label="Or paste a CSR" hint="When you have a CSR, its key stays on your server and only the certificate is issued."><Textarea rows={4} className="font-mono text-[11px]" value={f.csr_pem} onChange={(e) => setF({ ...f, csr_pem: e.target.value })} /></Field>
        </div>
      )}
    </Dialog>
  );
}

// ------------------------------------------------------------------ RADIUS
interface RadiusClient { name: string; address: string; secret: string; required_group: string }
interface RadiusView { config: { enabled: boolean; listen: string; clients: RadiusClient[] }; status: Status }

function Radius() {
  const toast = useToast();
  const v = useQuery<RadiusView>("/radius");
  const [cfg, setCfg] = useState<RadiusView["config"] | null>(null);
  useEffect(() => { if (v.data) setCfg(v.data.config); }, [v.data]);
  if (!v.data || !cfg) return <Loading />;
  const setClient = (i: number, p: Partial<RadiusClient>) => setCfg({ ...cfg, clients: cfg.clients.map((c, j) => (j === i ? { ...c, ...p } : c)) });
  return (
    <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_360px]">
      <Card title="RADIUS clients" subtitle="VPN gateways, switches, firewalls and captive portals that ask this server to check passwords"
        actions={<Button size="sm" icon={<Plus className="size-3.5" />} onClick={() => setCfg({ ...cfg, clients: [...cfg.clients, { name: "VPN gateway", address: "", secret: "", required_group: "" }] })}>Add client</Button>}>
        <div className="space-y-3">
          {cfg.clients.length === 0 && <p className="text-sm text-ink-3">No clients yet.</p>}
          {cfg.clients.map((c, i) => (
            <div key={i} className="grid items-end gap-2 sm:grid-cols-[1fr_1fr_1fr_1fr_auto]">
              <Field label="Name"><Input value={c.name} onChange={(e) => setClient(i, { name: e.target.value })} /></Field>
              <Field label="IP or range"><Input value={c.address} placeholder="10.0.0.1" onChange={(e) => setClient(i, { address: e.target.value })} /></Field>
              <Field label="Shared secret"><Input type="password" value={c.secret} onFocus={() => c.secret === KEEP && setClient(i, { secret: "" })} onChange={(e) => setClient(i, { secret: e.target.value })} /></Field>
              <Field label="Only this group"><Input value={c.required_group} placeholder="VPN Users" onChange={(e) => setClient(i, { required_group: e.target.value })} /></Field>
              <button className="mb-2 text-ink-3 hover:text-bad" onClick={() => setCfg({ ...cfg, clients: cfg.clients.filter((_, j) => j !== i) })} aria-label="Remove"><Trash2 className="size-4" /></button>
            </div>
          ))}
        </div>
      </Card>
      <Card title="RADIUS server" actions={<StatusBadge s={v.data.status} enabled={v.data.config.enabled} />}>
        <div className="space-y-3">
          {v.data.status.error && <Callout tone="warn">{v.data.status.error}</Callout>}
          <Switch checked={cfg.enabled} onChange={(x) => setCfg({ ...cfg, enabled: x })} label="Answer RADIUS requests" />
          <Field label="Listen on"><Input value={cfg.listen} onChange={(e) => setCfg({ ...cfg, listen: e.target.value })} /></Field>
          <Button variant="primary" onClick={async () => { try { const st = await put<Status>("/radius", cfg); toast(st.error ? "bad" : "good", st.error || "RADIUS settings saved"); void v.reload(); } catch (e) { toast("bad", (e as Error).message); } }}>Save and apply</Button>
          <p className="flex gap-1.5 text-xs text-ink-3"><AlertTriangle className="mt-0.5 size-3.5 shrink-0" />Supports PAP and MS-CHAPv2 (most VPNs and network devices). WPA2/3-Enterprise Wi-Fi needs EAP (PEAP or EAP-TLS), which isn't supported yet.</p>
        </div>
      </Card>
    </div>
  );
}

export function useCaRoot() {
  return useQuery<CaView>("/ca");
}
