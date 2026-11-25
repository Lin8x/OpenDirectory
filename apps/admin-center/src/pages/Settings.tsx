import { useEffect, useRef, useState } from "react";
import { put } from "../lib/api";
import { useBranding, type Branding } from "../lib/branding";
import { RefreshCw, Server } from "lucide-react";
import { autostartEnabled, setAutostart } from "../lib/autostart";
import { friendlyUpdateError, useUpdates } from "../lib/updates";
import { ago } from "../lib/format";
import { desktop, isDesktop } from "../lib/api";
import { useLearnMode, useSession } from "../lib/session";
import { Button, Card, CodeBlock, Explain, Field, Input, PageHeader, Select, Switch, useToast } from "../components/ui";

export default function Settings() {
  const { me, connection, setConnection } = useSession();
  const { learn, setLearn, theme, setTheme } = useLearnMode();
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
            <Field label="Theme">
              <Select value={theme} onChange={(e) => setTheme(e.target.value as "light" | "dark" | "system")} className="w-48">
                <option value="system">Match system</option><option value="light">Light</option><option value="dark">Dark</option>
              </Select>
            </Field>
          </div>
        </Card>
        <BrandingCard />
        <UpdatesCard />
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
