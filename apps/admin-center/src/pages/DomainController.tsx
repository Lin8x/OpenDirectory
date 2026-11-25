// Domain Controller: an experimental, Samba-backed AD DC role (Linux only).
// OpenDirectory stays the source of truth and syncs users, groups and
// passwords into Samba, so Windows, macOS and Linux can join the domain
// with their built-in tools and sign in with Kerberos.
import { Play, RefreshCw, Server } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router-dom";
import { useQuery } from "../lib/hooks";
import { post } from "../lib/api";
import { ago } from "../lib/format";
import { Badge, Button, Callout, Card, CodeBlock, Field, Input, Loading, PageHeader, Select, Tabs, useToast } from "../components/ui";

interface DcStatus {
  supported_os: boolean; samba_installed: boolean; samba_version: string; provisioned: boolean; running: boolean;
  realm: string; last_sync: string | null; last_sync_message: string; conflicts: string[];
}
type Info = { fsmo: string; sites: string; trusts: string; replication: string; level: string };
type Tab = "fsmo" | "sites" | "replication" | "trusts" | "join";
const ROLES = ["all", "rid", "pdc", "infrastructure", "schema", "naming", "domaindns", "forestdns"];

export default function DomainController() {
  const st = useQuery<DcStatus>("/dc");
  const s = st.data;
  return (
    <>
      <PageHeader title="Domain Controller" subtitle={<>Run a real Active Directory domain controller (Samba) on this Management Server, kept in sync with OpenDirectory. <Badge tone="warn">Experimental</Badge></>} />
      {!s ? <Loading /> : !s.supported_os ? (
        <Callout tone="warn" title="Linux only">The domain controller role needs a Linux Management Server with Samba installed. Windows and macOS servers can still use LDAP, RADIUS and the agent.</Callout>
      ) : !s.samba_installed ? <Install /> : !s.provisioned || !s.realm ? <Provision status={s} onDone={() => void st.reload()} /> : <Running status={s} onChange={() => void st.reload()} />}
    </>
  );
}

function Install() {
  return (
    <Card title="Install Samba first">
      <p className="mb-3 text-sm text-ink-2">Samba isn't installed on this server. Install it, then reload this page.</p>
      <CodeBlock label="Debian or Ubuntu" code="sudo apt install samba winbind krb5-user smbclient" />
      <div className="h-3" />
      <CodeBlock label="Fedora or RHEL (needs the Samba DC packages)" code="sudo dnf install samba-dc samba-client krb5-workstation" />
      <p className="mt-3 text-xs text-ink-3">The Management Server must run as root (it does when installed as a service) to provision and manage Samba.</p>
    </Card>
  );
}

function Provision({ status, onDone }: { status: DcStatus; onDone: () => void }) {
  const toast = useToast();
  const [pw, setPw] = useState("");
  const [fwd, setFwd] = useState("1.1.1.1");
  const [busy, setBusy] = useState(false);
  return (
    <div className="grid gap-5 lg:grid-cols-[1fr_340px]">
      <Card title="Promote this server to a domain controller" subtitle={status.samba_version}>
        {status.conflicts.map((c) => <div key={c} className="mb-3"><Callout tone="warn">{c} <Link className="underline" to="/network-services">Open Network Services</Link></Callout></div>)}
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Domain Administrator password" hint="At least 12 characters. This is the built-in Administrator account in Samba."><Input type="password" value={pw} onChange={(e) => setPw(e.target.value)} /></Field>
          <Field label="DNS forwarder" hint="Where the DC sends lookups for names outside the domain."><Input value={fwd} onChange={(e) => setFwd(e.target.value)} /></Field>
        </div>
        <div className="mt-4 flex justify-end">
          <Button variant="primary" loading={busy} disabled={pw.length < 12 || status.conflicts.length > 0} icon={<Play className="size-4" />} onClick={async () => {
            setBusy(true);
            try { const r = await post<{ sync: string }>("/dc/provision", { admin_password: pw, dns_forwarder: fwd }); toast("good", `Domain provisioned. ${r.sync}`); setPw(""); onDone(); }
            catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
          }}>Provision domain</Button>
        </div>
      </Card>
      <Card title="What this does">
        <ul className="list-disc space-y-1.5 pl-4 text-sm text-ink-2">
          <li>Runs <code>samba-tool domain provision</code> for your domain, with Samba's internal DNS.</li>
          <li>Starts the <code>samba-ad-dc</code> service (LDAP, Kerberos, DNS, SMB for SYSVOL).</li>
          <li>Copies your users, groups and memberships into Samba, then keeps them in sync every 5 minutes. Passwords sync the next time each user signs in or changes it.</li>
          <li>Objects created directly in Samba aren't touched.</li>
        </ul>
      </Card>
    </div>
  );
}

function Running({ status, onChange }: { status: DcStatus; onChange: () => void }) {
  const toast = useToast();
  const info = useQuery<Info>("/dc/info");
  const [tab, setTab] = useState<Tab>("fsmo");
  const [syncing, setSyncing] = useState(false);
  const act = async (action: string, args: string[]) => {
    try { const r = await post<{ ok: boolean; output: string }>("/dc/action", { action, args }); toast(r.ok ? "good" : "bad", r.output || (r.ok ? "Done" : "Failed")); void info.reload(); }
    catch (e) { toast("bad", (e as Error).message); }
  };
  return (
    <>
      <div className="mb-5 grid gap-4 sm:grid-cols-3">
        <Card><div className="flex items-center gap-3"><Server className="size-5 text-ink-3" /><div><div className="text-xs text-ink-3">Realm</div><div className="font-medium">{status.realm}</div></div></div></Card>
        <Card><div className="text-xs text-ink-3">samba-ad-dc service</div><div className="mt-1"><Badge tone={status.running ? "good" : "bad"}>{status.running ? "Running" : "Stopped"}</Badge></div></Card>
        <Card>
          <div className="flex items-start justify-between gap-2">
            <div><div className="text-xs text-ink-3">Directory sync</div><div className="text-sm">{status.last_sync ? `${ago(status.last_sync)}: ${status.last_sync_message}` : "Not synced yet"}</div></div>
            <Button size="sm" loading={syncing} icon={<RefreshCw className="size-3.5" />} onClick={async () => {
              setSyncing(true);
              try { const r = await post<{ message: string }>("/dc/sync"); toast("good", r.message); onChange(); } catch (e) { toast("bad", (e as Error).message); } finally { setSyncing(false); }
            }}>Sync now</Button>
          </div>
        </Card>
      </div>
      {status.conflicts.map((c) => <div key={c} className="mb-3"><Callout tone="warn">{c}</Callout></div>)}
      <Card padded={false}>
        <div className="border-b border-line px-4 pt-3">
          <Tabs<Tab> value={tab} onChange={setTab} tabs={[{ id: "fsmo", label: "FSMO roles" }, { id: "sites", label: "Sites and subnets" }, { id: "replication", label: "Replication" }, { id: "trusts", label: "Trusts" }, { id: "join", label: "Join computers" }]} />
        </div>
        <div className="p-4">
          {tab === "join" ? <Join realm={status.realm} /> : !info.data ? <Loading /> : (
            <>
              {tab === "fsmo" && <><Pre text={info.data.fsmo} /><Pre text={info.data.level} /><RoleActions onRun={act} /></>}
              {tab === "sites" && <><Pre text={info.data.sites} /><SiteActions onRun={act} /></>}
              {tab === "replication" && <><Pre text={info.data.replication} /><ReplicateAction onRun={act} /><p className="mt-3 text-xs text-ink-3">To add a second DC, join another Linux server with <code>samba-tool domain join {status.realm.toLowerCase()} DC -U Administrator</code>. Multi-DC setups are managed with samba-tool; OpenDirectory syncs to this DC and Samba replicates the rest.</p></>}
              {tab === "trusts" && <><Pre text={info.data.trusts} /><p className="mt-3 text-xs text-ink-3">Create trusts with <code>samba-tool domain trust create</code>. Samba supports external and forest trusts with Active Directory domains; there are known limits (for example no selective authentication).</p></>}
            </>
          )}
        </div>
      </Card>
    </>
  );
}

function Pre({ text }: { text: string }) {
  return <pre className="mb-3 max-h-72 overflow-auto rounded-lg bg-panel-2 p-3 font-mono text-xs whitespace-pre-wrap">{text || "Nothing to show."}</pre>;
}

function RoleActions({ onRun }: { onRun: (a: string, args: string[]) => Promise<void> }) {
  const [role, setRole] = useState("all");
  return (
    <div className="flex flex-wrap items-end gap-2">
      <Field label="Role"><Select value={role} onChange={(e) => setRole(e.target.value)}>{ROLES.map((r) => <option key={r}>{r}</option>)}</Select></Field>
      <Button onClick={() => void onRun("fsmo_transfer", [role])}>Transfer to this DC</Button>
      <Button variant="danger" onClick={() => { if (confirm("Seize only when the current role holder is gone for good. Continue?")) void onRun("fsmo_seize", [role]); }}>Seize</Button>
    </div>
  );
}

function SiteActions({ onRun }: { onRun: (a: string, args: string[]) => Promise<void> }) {
  const [site, setSite] = useState("");
  const [subnet, setSubnet] = useState("");
  return (
    <div className="flex flex-wrap items-end gap-2">
      <Field label="Site name"><Input value={site} onChange={(e) => setSite(e.target.value)} placeholder="Branch-Office" /></Field>
      <Button disabled={!site} onClick={() => void onRun("site_create", [site])}>Create site</Button>
      <Field label="Subnet"><Input value={subnet} onChange={(e) => setSubnet(e.target.value)} placeholder="10.2.0.0/16" /></Field>
      <Button disabled={!site || !subnet} onClick={() => void onRun("subnet_create", [subnet, site])}>Add subnet to site</Button>
    </div>
  );
}

function ReplicateAction({ onRun }: { onRun: (a: string, args: string[]) => Promise<void> }) {
  const [to, setTo] = useState("");
  const [from, setFrom] = useState("");
  const [nc, setNc] = useState("");
  return (
    <div className="flex flex-wrap items-end gap-2">
      <Field label="Destination DC"><Input value={to} onChange={(e) => setTo(e.target.value)} /></Field>
      <Field label="Source DC"><Input value={from} onChange={(e) => setFrom(e.target.value)} /></Field>
      <Field label="Naming context"><Input value={nc} onChange={(e) => setNc(e.target.value)} placeholder="DC=corp,DC=example,DC=com" /></Field>
      <Button disabled={!to || !from || !nc} onClick={() => void onRun("replicate", [to, from, nc])}>Replicate now</Button>
    </div>
  );
}

function Join({ realm }: { realm: string }) {
  const domain = realm.toLowerCase();
  return (
    <div className="space-y-4 text-sm">
      <p className="text-ink-2">Point each computer's DNS at this server, then join it. The Script Library has ready-made <Link className="underline" to="/scripts">domain join templates</Link> you can run on enrolled endpoints, with the password kept out of job history.</p>
      <CodeBlock label="Windows (PowerShell, as administrator)" code={`Add-Computer -DomainName ${domain} -Credential ${domain.split(".")[0].toUpperCase()}\\Administrator -Restart`} />
      <CodeBlock label="Linux (realmd and SSSD)" code={`sudo realm join --user=Administrator ${domain}`} />
      <CodeBlock label="macOS" code={`sudo dsconfigad -add ${domain} -computer "$(scutil --get LocalHostName)" -username Administrator -mobile enable`} />
    </div>
  );
}
