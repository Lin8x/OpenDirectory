import { RefreshCw, Server } from "lucide-react";
import { friendlyUpdateError, useUpdates } from "../lib/updates";
import { ago } from "../lib/format";
import { desktop, isDesktop } from "../lib/api";
import { useLearnMode, useSession } from "../lib/session";
import { Button, Card, CodeBlock, Explain, Field, PageHeader, Select, Switch } from "../components/ui";

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
        <UpdatesCard />
      </div>
    </>
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
