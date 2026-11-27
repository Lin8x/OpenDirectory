import { useEffect, useRef, useState } from "react";
import { post, put } from "../lib/api";
import { useQuery } from "../lib/hooks";
import type { OrgUnit } from "../lib/types";
import { useBranding, type Branding } from "../lib/branding";
import { RefreshCw, Server } from "lucide-react";
import { autostartEnabled, setAutostart } from "../lib/autostart";
import { friendlyUpdateError, useUpdates } from "../lib/updates";
import { ago } from "../lib/format";
import { desktop, isDesktop } from "../lib/api";
import { useLearnMode, useSession } from "../lib/session";
import { Badge, Button, Card, CodeBlock, Explain, Field, Input, PageHeader, Select, Switch, useToast } from "../components/ui";

export default function Settings() {
  const { me, connection, setConnection } = useSession();
  const { learn, setLearn, theme, setTheme, ui, setUi } = useLearnMode();
  const s = me?.server;
  return (
    <>
      <PageHeader title="Server & connection" subtitle="Details other admins and endpoints need to reach this Management Server, and your Admin Center preferences." />
      <div className="grid gap-5 lg:grid-cols-2">
        <Card title={<span className="flex items-center gap-2"><Server className="size-4" />Management Server</span>}>
          <dl className="space-y-3 text-sm">
            <Row k="Organization" v={me?.organization} />
            <Row k="Domain" v={me?.domain} />
            <Row k="Version" v={s?.version} />
            <Row k="Data directory" v={<span className="selectable font-mono text-xs">{s?.data_dir}</span>} />
            <Row k="Hosted" v={connection?.mode === "local" ? "On this computer" : connection?.mode === "remote" ? "Remote server" : "Development server"} />
          </dl>
          {isDesktop && <Button className="mt-4" onClick={async () => setConnection(await desktop.disconnect())}>Disconnect / change server</Button>}
        </Card>
        <Card title="Connect another Admin Center or od CLI">
          <div className="space-y-3">
            <CodeBlock label="Server address" code={s?.endpoint_url ?? ""} />
            {s?.tls_fingerprint && <CodeBlock label="TLS fingerprint" code={s.tls_fingerprint} />}
            <CodeBlock label="od CLI" code={`od login --server ${s?.endpoint_url} ${s?.tls_fingerprint ? `--fingerprint ${s.tls_fingerprint} ` : ""}--username ${me?.user.username}`} />
            <Explain term="fingerprint" />
          </div>
        </Card>
        <Card title="Preferences">
          <div className="space-y-4">
            <Switch checked={learn} onChange={setLearn} label="Learn mode: show explanations throughout the app" />
            <Field label="Interface" hint="Classic mimics the console layout of Active Directory Users and Computers and Group Policy Management: a tree, a details list, right-click menus and property sheets.">
              <Select value={ui} onChange={(e) => setUi(e.target.value as "modern" | "classic")} className="w-48">
                <option value="modern">Modern</option><option value="classic">Classic (console)</option>
              </Select>
            </Field>
            <Field label="Theme">
              <Select value={theme} onChange={(e) => setTheme(e.target.value as "light" | "dark" | "system")} className="w-48">
                <option value="system">Match system</option><option value="light">Light</option><option value="dark">Dark</option>
              </Select>
            </Field>
          </div>
        </Card>
        <BrandingCard />
        <UpdatesCard />
        <AgentUpdatesCard />
        <InternetGatewayCard />
        <StartupCard hosting={connection?.mode === "local"} />
      </div>
    </>
  );
}

function BrandingCard() {
  const { brand, reload, preview } = useBranding();
  const toast = useToast();
  const [b, setB] = useState<Branding>(brand);
  const [busy, setBusy] = useState(false);
  const file = useRef<HTMLInputElement>(null);
  useEffect(() => setB(brand), [brand.logo, brand.accent, brand.product_name, brand.support_contact]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => preview(null), [preview]);
  const change = (n: Branding) => { setB(n); preview(n); };
  return (
    <Card title="Branding" subtitle="Your logo, color and name across the Admin Center and its sign-in screen">
      <div className="space-y-4">
        <div className="flex items-center gap-4">
          <div className="grid size-16 place-items-center rounded-xl border border-line bg-panel-2"><img src={b.logo || "/logo.svg"} className="max-h-12 max-w-12 object-contain" alt="Logo preview" /></div>
          <div className="flex flex-wrap gap-2">
            <input ref={file} type="file" accept="image/png,image/jpeg,image/webp,image/svg+xml" className="hidden" onChange={(e) => {
              const f = e.target.files?.[0]; if (!f) return;
              if (f.size > 512 * 1024) { toast("bad", "Logos are limited to 512 KB"); return; }
              const r = new FileReader(); r.onload = () => change({ ...b, logo: String(r.result) }); r.readAsDataURL(f);
            }} />
            <Button size="sm" onClick={() => file.current?.click()}>Upload logo</Button>
            {b.logo && <Button size="sm" variant="ghost" onClick={() => change({ ...b, logo: "" })}>Use default</Button>}
          </div>
        </div>
        <p className="text-xs text-ink-3">PNG, JPEG, WebP or SVG up to 512 KB. A square logo on a transparent background looks best.</p>
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Accent color">
            <div className="flex items-center gap-2">
              <input type="color" value={b.accent || "#3a63f3"} onChange={(e) => change({ ...b, accent: e.target.value })} className="h-9 w-12 cursor-pointer rounded border border-line bg-panel" aria-label="Accent color" />
              <Input value={b.accent} placeholder="#3a63f3" onChange={(e) => change({ ...b, accent: e.target.value })} className="font-mono" />
            </div>
          </Field>
          <Field label="Product name" hint="Shown instead of “Admin Center”"><Input value={b.product_name} placeholder="Contoso IT" onChange={(e) => change({ ...b, product_name: e.target.value })} /></Field>
        </div>
        <Field label="Support contact" hint="Shown on the sign-in screen, e.g. an email or phone number"><Input value={b.support_contact} placeholder="it-help@contoso.com" onChange={(e) => change({ ...b, support_contact: e.target.value })} /></Field>
        <div className="flex gap-2">
          <Button variant="primary" loading={busy} onClick={async () => {
            setBusy(true);
            try { await put("/branding", b); preview(null); reload(); toast("good", "Branding saved"); } catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
          }}>Save branding</Button>
          <Button variant="ghost" onClick={() => { preview(null); setB(brand); }}>Reset</Button>
        </div>
      </div>
    </Card>
  );
}

function StartupCard({ hosting }: { hosting: boolean }) {
  const [on, setOn] = useState<boolean | null>(null);
  const toast = useToast();
  useEffect(() => { void autostartEnabled().then(setOn); }, []);
  if (!isDesktop) return null;
  return (
    <Card title="Startup">
      <div className="space-y-3">
        <Switch checked={!!on} disabled={on === null} onChange={async (v) => {
          try { await setAutostart(v); setOn(v); toast("good", v ? "The Admin Center will start when you sign in" : "Launch at startup turned off"); } catch (e) { toast("bad", String(e)); }
        }} label="Launch the Admin Center at startup (in the tray)" />
        {hosting && !on && <p className="text-xs text-warn">This computer hosts the directory. Without launch at startup, endpoints can't check in after a restart until someone opens the app.</p>}
        <p className="text-xs text-ink-3">On a server that runs with nobody signed in, install the Management Server as a system service instead:</p>
        <CodeBlock code="sudo opendirectory-server install-service" />
        <p className="text-xs text-ink-3">Windows: run <code className="font-mono">opendirectory-server.exe install-service</code> from an elevated prompt. Then create the domain with <code className="font-mono">od init</code> or connect this Admin Center to it.</p>
      </div>
    </Card>
  );
}

function UpdatesCard() {
  const u = useUpdates();
  if (!u.supported) return null;
  const line =
    u.status === "checking" ? "Checking…" :
    u.status === "up-to-date" ? `You're up to date (v${u.currentVersion}).` :
    u.status === "available" ? `Version ${u.available?.version} is available.` :
    u.status === "downloading" ? "Downloading the update…" :
    u.status === "error" ? friendlyUpdateError(u.error) : null;
  return (
    <Card title="Updates">
      <div className="space-y-4">
        <dl className="space-y-3 text-sm">
          <Row k="Admin Center version" v={u.currentVersion} />
          <Row k="Status" v={u.status === "up-to-date" ? <span className="text-good">Up to date</span> : u.status === "available" ? <span className="text-accent">Update available: {u.available?.version}</span> : u.status === "error" ? <span className="text-warn">Unknown</span> : u.status === "checking" ? "Checking…" : "Not checked yet"} />
          <Row k="Last checked" v={u.lastChecked ? ago(new Date(u.lastChecked).toISOString()) : "Never"} />
        </dl>
        <Switch checked={u.autoCheck} onChange={u.setAutoCheck} label="Check for updates when the app opens" />
        <div className="flex items-center gap-3">
          {u.status === "available" ? (
            <Button variant="primary" onClick={u.openPrompt}>Install {u.available?.version}</Button>
          ) : (
            <Button icon={<RefreshCw className="size-4" />} loading={u.status === "checking"} onClick={u.checkNow}>Check for updates</Button>
          )}
          {line && <span className="text-sm text-ink-3">{line}</span>}
        </div>
        <p className="text-xs text-ink-3">Updates come from signed OpenDirectory releases on GitHub. The signature is verified before anything is installed.</p>
      </div>
    </Card>
  );
}

function Row({ k, v }: { k: string; v: React.ReactNode }) {
  return <div className="flex justify-between gap-4"><dt className="shrink-0 text-ink-3">{k}</dt><dd className="min-w-0 break-all text-right">{v ?? "—"}</dd></div>;
}

interface AgentUpdateStatus {
  policy: { mode: "off" | "pilot" | "all"; pilot_ou_ids: string[] };
  version: string;
  builds: { platform: string; cached: boolean; size?: number }[];
  versions: [string, number][];
  outdated: number;
  manual: number;
}

/** Endpoint Agent updates: rollout ring and build status. */
function AgentUpdatesCard() {
  const st = useQuery<AgentUpdateStatus>("/agent-updates");
  const ous = useQuery<OrgUnit[]>("/ous");
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const s = st.data;
  if (!s) return null;
  const save = async (p: AgentUpdateStatus["policy"]) => {
    try { await put("/agent-updates", p); toast("good", "Agent update ring saved"); void st.reload(); } catch (e) { toast("bad", (e as Error).message); }
  };
  return (
    <Card title="Endpoint Agent updates" subtitle={`Agents older than ${s.version} update themselves from this server, then restart. A failed update rolls back on its own.`}>
      <div className="grid gap-4 md:grid-cols-2">
        <Field label="Rollout ring">
          <Select value={s.policy.mode} onChange={(e) => {
            const mode = e.target.value as AgentUpdateStatus["policy"]["mode"];
            if (mode === "pilot" && s.policy.pilot_ou_ids.length === 0) { st.setData?.({ ...s, policy: { ...s.policy, mode } }); return; }
            void save({ ...s.policy, mode });
          }}>
            <option value="all">All endpoints (automatic)</option>
            <option value="pilot">Pilot OUs only</option>
            <option value="off">Off (update agents yourself)</option>
          </Select>
        </Field>
        {s.policy.mode === "pilot" && (
          <Field label="Pilot OUs" hint="Endpoints in these OUs (and their child OUs) update first. Switch to All endpoints when you're happy.">
            <div className="max-h-40 space-y-1 overflow-y-auto rounded-lg border border-line p-2">
              {(ous.data ?? []).map((o) => (
                <label key={o.id} className="flex items-center gap-2 text-sm">
                  <input type="checkbox" checked={s.policy.pilot_ou_ids.includes(o.id)} onChange={(e) => {
                    const ids = e.target.checked ? [...s.policy.pilot_ou_ids, o.id] : s.policy.pilot_ou_ids.filter((x) => x !== o.id);
                    if (ids.length) void save({ mode: "pilot", pilot_ou_ids: ids }); else st.setData?.({ ...s, policy: { mode: "pilot", pilot_ou_ids: [] } });
                  }} />{o.name}
                </label>
              ))}
              {(ous.data ?? []).length === 0 && <p className="text-xs text-ink-3">Create an OU first.</p>}
            </div>
          </Field>
        )}
      </div>
      <div className="mt-4 flex flex-wrap items-center gap-2 text-sm">
        <span className="text-ink-3">Agent builds for {s.version}:</span>
        {s.builds.map((b) => <Badge key={b.platform} tone={b.cached ? "good" : "neutral"}>{b.platform}{b.cached ? "" : " (not downloaded)"}</Badge>)}
        <Button size="sm" loading={busy} icon={<RefreshCw className="size-3.5" />} onClick={async () => {
          setBusy(true);
          try {
            const r = await post<{ platform: string; ok: boolean; error?: string }[]>("/agent-updates/fetch");
            const bad = r.filter((x) => !x.ok);
            toast(bad.length ? "bad" : "good", bad.length ? `Couldn't download: ${bad.map((b) => b.platform).join(", ")}. ${bad[0]?.error ?? ""}` : "Downloaded every agent build");
            void st.reload();
          } finally { setBusy(false); }
        }}>Download now</Button>
      </div>
      <div className="mt-3 text-sm">
        {s.outdated === 0 ? <span className="text-good">Every endpoint runs agent {s.version}.</span> : <span>{s.outdated} endpoint(s) run an older agent: {s.versions.filter(([v]) => v !== s.version).map(([v, n]) => `${n} on ${v}`).join(", ")}.</span>}
      </div>
      {s.manual > 0 && <div className="mt-2"><Explain>{s.manual} endpoint(s) run an agent from before automatic updates (0.11 or older). Update those once by running the install command again; after that they update themselves. Builds download on demand from the GitHub release; on a network without internet access, upload them with POST /api/v1/agent-updates/upload/&lt;platform&gt;.</Explain></div>}
    </Card>
  );
}

interface GatewayView { enabled: boolean; port: number; public_url: string; status: string; tls_fingerprint: string | null; devices_with_certificates: number; endpoints: number }

/** Internet Gateway: agents outside the office reach this server over mutual TLS. */
function InternetGatewayCard() {
  const g = useQuery<GatewayView>("/internet-gateway");
  const toast = useToast();
  const [draft, setDraft] = useState<{ port: number; public_url: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const v = g.data;
  if (!v) return null;
  const d = draft ?? { port: v.port, public_url: v.public_url };
  const save = async (enabled: boolean) => {
    setBusy(true);
    try {
      const r = await put<GatewayView>("/internet-gateway", { enabled, ...d });
      g.setData?.(r); setDraft(null);
      toast(r.status.startsWith("Listening") || !enabled ? "good" : "bad", enabled ? r.status : "Internet Gateway turned off");
    } catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
  };
  return (
    <Card title="Internet access for agents" subtitle="Let laptops and phones check in from home or on the road, not only on the office network.">
      <div className="space-y-3 text-sm">
        <div className="flex flex-wrap items-center gap-2">
          <Badge tone={v.enabled && v.status.startsWith("Listening") ? "good" : v.enabled ? "bad" : "neutral"}>{v.enabled ? v.status : "Off"}</Badge>
          <span className="text-ink-3">{v.devices_with_certificates} of {v.endpoints} endpoints have a device certificate</span>
        </div>
        <div className="grid gap-3 sm:grid-cols-[1fr_120px]">
          <Field label="Public address" hint="The DNS name or public IP your router forwards to this server, with the port.">
            <Input value={d.public_url} placeholder="https://od.example.com:8444" onChange={(e) => setDraft({ ...d, public_url: e.target.value })} />
          </Field>
          <Field label="Port"><Input type="number" value={d.port} onChange={(e) => setDraft({ ...d, port: Number(e.target.value) })} /></Field>
        </div>
        <ol className="list-decimal space-y-1 pl-5 text-ink-2">
          <li>On your router or firewall, forward TCP port {d.port} to this server.</li>
          <li>Enter the public address and turn it on.</li>
          <li>Agents learn the address at their next check-in and use it whenever the office network can't be reached. New devices can also enroll with it.</li>
        </ol>
        <Explain>The gateway serves only Endpoint Agents, never the Admin Center or sign-in. Each agent gets a device certificate from a built-in Device Identity CA and must present it (mutual TLS), and the agent pins this server's certificate fingerprint, so no public certificate or VPN is needed. Retiring an endpoint revokes its certificate.</Explain>
        <div className="flex gap-2">
          {v.enabled ? <>
            <Button variant="primary" loading={busy} disabled={!draft} onClick={() => save(true)}>Save</Button>
            <Button loading={busy} onClick={() => save(false)}>Turn off</Button>
          </> : <Button variant="primary" loading={busy} disabled={!d.public_url.trim()} onClick={() => save(true)}>Turn on</Button>}
        </div>
      </div>
    </Card>
  );
}
