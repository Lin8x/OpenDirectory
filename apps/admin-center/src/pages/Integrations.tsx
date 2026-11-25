// Integrations: connect OpenDirectory to existing Windows services (AD, Entra
// ID, SMB, DNS, DHCP, WSUS…) and to cloud apps and alerting.
import { useState } from "react";
import { useSearchParams } from "react-router-dom";
import { Bell, Building2, CheckCircle2, Cloud, DownloadCloud, FileKey2, Globe, HardDrive, Network, Pencil, Plug, Printer, RefreshCw, Server, Trash2, Webhook, XCircle } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { del, post, put } from "../lib/api";
import { ago } from "../lib/format";
import { Badge, Button, Callout, Card, Confirm, Dialog, Explain, Field, Input, PageHeader, Switch, Tabs, cx, useToast } from "../components/ui";

const KEEP = "••••••••";
type Conn = { type: string } & Record<string, unknown>;
interface Integration { id: string; name: string; kind: string; connector: Conn; enabled: boolean; can_import: boolean; last_sync_at: string | null; last_ok: boolean | null; last_message: string }
type F = [string, string, "text" | "secret" | "bool" | "number" | "list", string?];
interface Def { id: string; title: string; icon: React.ReactNode; blurb: string; status: "import" | "test" | "alerts" | "guide"; connector?: Conn; fields?: F[]; steps: string[]; note?: string }

const WINDOWS: Def[] = [
  { id: "ad", title: "Active Directory domain", icon: <Building2 className="size-4" />, status: "import", blurb: "Import users and security groups from an existing AD or Samba AD domain controller over LDAPS, so both directories line up.",
    connector: { type: "active_directory", url: "ldaps://", bind_dn: "", password: "", base_dn: "", starttls: false, skip_tls_verify: false },
    fields: [["url", "Domain controller URL", "text", "ldaps://dc01.corp.contoso.com:636"], ["bind_dn", "Bind account", "text", "svc-opendirectory@corp.contoso.com"], ["password", "Password", "secret"], ["base_dn", "Base DN", "text", "DC=corp,DC=contoso,DC=com"], ["starttls", "Use StartTLS on port 389", "bool"], ["skip_tls_verify", "Accept a self-signed certificate (lab only)", "bool"]],
    steps: [
      "On a domain controller, create a service account (for example svc-opendirectory) in Active Directory Users and Computers. A normal Domain User can read users and groups; it doesn't need admin rights.",
      "Make sure LDAPS (TCP 636) is reachable from this server. If the DC has no certificate yet, install AD CS or a certificate for the DC, or use StartTLS on 389.",
      "Find your base DN: it's your domain in DC= form, e.g. corp.contoso.com becomes DC=corp,DC=contoso,DC=com.",
      "Fill in the form, click Test, then Import. Users land in an \"Imported from …\" OU with their groups. Passwords can't be copied from AD, so set a password for each person who should sign in through OpenDirectory.",
    ],
    note: "Two-way sync and acting as a domain controller come with the Domain Controller role on the roadmap." },
  { id: "entra", title: "Microsoft Entra ID / Microsoft 365", icon: <Cloud className="size-4" />, status: "import", blurb: "Import users and security groups from Entra ID (Azure AD) through Microsoft Graph.",
    connector: { type: "entra_id", tenant_id: "", client_id: "", client_secret: "" },
    fields: [["tenant_id", "Directory (tenant) ID", "text"], ["client_id", "Application (client) ID", "text"], ["client_secret", "Client secret", "secret"]],
    steps: [
      "In the Microsoft Entra admin center, open App registrations and create a new registration named OpenDirectory.",
      "Under API permissions, add Microsoft Graph application permissions User.Read.All, GroupMember.Read.All and Organization.Read.All, then click Grant admin consent.",
      "Under Certificates & secrets, create a client secret and copy its value.",
      "Copy the Directory (tenant) ID and Application (client) ID from the Overview page, fill in the form, Test, then Import.",
    ] },
  { id: "smb", title: "SMB file server", icon: <HardDrive className="size-4" />, status: "test", blurb: "Check a Windows or Samba file server is reachable, then map its shares with a sign-in script.",
    connector: { type: "windows_service", service: "smb", host: "", port: 445 }, fields: [["host", "File server", "text", "fs01.corp.contoso.com"], ["port", "Port", "number"]],
    steps: ["Enter the file server's name and click Test to check TCP 445 is open from here.", "In the Script Library, add the \"Map a network drive (sign-in script)\" template and set the share path, e.g. \\\\fs01\\shared.", "Attach the script to a GPO as a sign-in script and link the GPO to the OU of the people who need the drive.", "Grant share and NTFS permissions on the server as usual. To use the server as a backup destination, mount the share and pick \"Folder or network share\" in Backup & Restore."] },
  { id: "dns", title: "DNS server", icon: <Globe className="size-4" />, status: "test", blurb: "Point endpoints at your existing Windows or BIND DNS server.",
    connector: { type: "windows_service", service: "dns", host: "", port: 53 }, fields: [["host", "DNS server", "text", "10.0.0.10"], ["port", "Port", "number"]],
    steps: ["Enter the DNS server's address and Test (checks TCP 53).", "Create an A record for this Management Server (e.g. od.corp.contoso.com) so agents can find it by name.", "Hand out the DNS server through DHCP, or set it on each endpoint with a startup script.", "Or use OpenDirectory's own DNS server in Network Services."] },
  { id: "dhcp", title: "DHCP server", icon: <Network className="size-4" />, status: "test", blurb: "Use your existing Windows DHCP server alongside OpenDirectory.",
    connector: { type: "windows_service", service: "dhcp", host: "", port: 67 }, fields: [["host", "DHCP server", "text"]],
    steps: ["Enter the DHCP server's name. The test checks it resolves; DHCP itself uses UDP broadcast, which can't be tested from here.", "Set DHCP option 006 (DNS servers) and 015 (DNS domain name) to match your OpenDirectory domain.", "Optionally reserve an address for this Management Server so its URL never changes.", "Or use OpenDirectory's own DHCP server in Network Services."] },
  { id: "wsus", title: "WSUS", icon: <DownloadCloud className="size-4" />, status: "test", blurb: "Keep using Windows Server Update Services for Windows patches.",
    connector: { type: "windows_service", service: "wsus", host: "", port: 8530 }, fields: [["host", "WSUS server", "text"], ["port", "Port (8530 HTTP, 8531 HTTPS)", "number"]],
    steps: ["Enter the WSUS server and Test (checks TCP 8530 or 8531).", "Point Windows endpoints at it with a startup script that sets WUServer and WUStatusServer under HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\WindowsUpdate and UseWUServer = 1 under its AU key.", "Use the Update deferral GPO setting for patch rings if you don't need WSUS approvals.", "Or use Patch Management in OpenDirectory to scan and install updates without WSUS."] },
  { id: "dfs", title: "DFS Namespaces", icon: <HardDrive className="size-4" />, status: "test", blurb: "Use an existing DFS namespace (Windows Server or Samba) for shared folders.",
    connector: { type: "windows_service", service: "smb", host: "", port: 445 }, fields: [["host", "Namespace server", "text", "corp.contoso.com"], ["port", "Port", "number"]],
    steps: ["Enter a namespace server (or the domain name for a domain-based namespace) and Test.", "Map the namespace with the \"Map a network drive\" template, e.g. \\\\corp.contoso.com\\shares, as a GPO sign-in script.", "Domain-based namespaces need a domain controller: use Windows Server, or the Samba-backed Domain Controller role (Samba supports standalone and domain DFS referrals).", "OpenDirectory doesn't replicate folders (DFS-R); use DFS-R on Windows, or rsync or Syncthing between Linux file servers."] },
  { id: "rds", title: "Remote Desktop Services", icon: <Server className="size-4" />, status: "test", blurb: "Check an RDS host or gateway is reachable.",
    connector: { type: "windows_service", service: "rds", host: "", port: 3389 }, fields: [["host", "RDS host or gateway", "text"], ["port", "Port", "number"]],
    steps: ["Enter the host and Test (checks TCP 3389, or 443 for a gateway).", "Use the Remote Desktop GPO setting to allow or block RDP on endpoints.", "OpenDirectory manages RDS hosts like any endpoint; it doesn't replace RDS session hosting."] },
  { id: "print", title: "Print server", icon: <Printer className="size-4" />, status: "test", blurb: "Check a Windows print server is reachable.",
    connector: { type: "windows_service", service: "print", host: "", port: 445 }, fields: [["host", "Print server", "text"]],
    steps: ["Enter the print server and Test.", "Deploy printers with a sign-in script, e.g. Add-Printer -ConnectionName \\\\print01\\Floor2 in PowerShell.", "Or use the \"Deploy a printer\" templates in the Script Library (Windows, macOS and Linux) as GPO sign-in or startup scripts. OpenDirectory doesn't host print queues itself."] },
  { id: "ca", title: "Certificate Authority (AD CS)", icon: <FileKey2 className="size-4" />, status: "test", blurb: "Check an Active Directory Certificate Services server is reachable.",
    connector: { type: "windows_service", service: "ca", host: "", port: 135 }, fields: [["host", "CA server", "text"]],
    steps: ["Enter the CA server and Test (checks RPC on TCP 135).", "Distribute the CA's root certificate to endpoints with a startup script.", "Or use OpenDirectory's own certificate authority in Network Services, with the Machine certificate auto-enrollment GPO setting."] },
];

const CLOUD: Def[] = [
  { id: "webhook", title: "Webhook", icon: <Webhook className="size-4" />, status: "alerts", blurb: "POST audit events as JSON to any URL, signed with HMAC-SHA256. Use it for a SIEM, a ticketing system or your own automation.",
    connector: { type: "webhook", url: "", secret: "", events: [] }, fields: [["url", "URL", "text", "https://siem.example.com/hooks/opendirectory"], ["secret", "Signing secret", "secret", "Optional"], ["events", "Events (prefixes, comma separated)", "list", "Empty for all, e.g. backup., endpoint.enroll, user."]],
    steps: ["Create an endpoint that accepts POST requests with a JSON body.", "Optionally set a secret and verify the X-OpenDirectory-Signature header (sha256=HMAC of the body).", "Choose which events to send by action prefix, or leave it empty for everything. Click Test to send a sample."] },
  { id: "slack", title: "Slack", icon: <Bell className="size-4" />, status: "alerts", blurb: "Post alerts, such as failed backups or new endpoints, to a Slack channel.",
    connector: { type: "slack", webhook_url: "", events: ["backup.failed", "endpoint.enroll", "auth.failed"] }, fields: [["webhook_url", "Incoming webhook URL", "secret", "https://hooks.slack.com/services/…"], ["events", "Events (prefixes, comma separated)", "list"]],
    steps: ["In Slack, create an app (or use an existing one) and turn on Incoming Webhooks.", "Add a webhook to the channel you want and copy its URL.", "Paste it here, choose events and click Test."] },
  { id: "teams", title: "Microsoft Teams", icon: <Bell className="size-4" />, status: "alerts", blurb: "Post alerts to a Teams channel.",
    connector: { type: "teams", webhook_url: "", events: ["backup.failed", "endpoint.enroll", "auth.failed"] }, fields: [["webhook_url", "Workflows webhook URL", "secret", "https://…logic.azure.com/…"], ["events", "Events (prefixes, comma separated)", "list"]],
    steps: ["In the Teams channel, open Workflows and choose \"Post to a channel when a webhook request is received\".", "Copy the URL it gives you.", "Paste it here, choose events and click Test."] },
  { id: "google", title: "Google Workspace", icon: <Cloud className="size-4" />, status: "guide", blurb: "User and group import from Google Workspace is planned.",
    steps: ["Google Workspace import needs a service account with domain-wide delegation; it's planned for a later release.", "Until then, export users from the Google Admin console as CSV and create them in OpenDirectory, or use the Webhook integration for alerts."] },
];

export default function Integrations() {
  const [params, setParams] = useSearchParams();
  const tab = params.get("tab") === "cloud" ? "cloud" : "windows";
  const list = useQuery<Integration[]>("/integrations");
  const [editing, setEditing] = useState<{ def: Def; existing?: Integration } | null>(null);
  const defs = tab === "windows" ? WINDOWS : CLOUD;
  const mine = (list.data ?? []).filter((i) => defs.some((d) => d.connector && matches(d, i)));
  return (
    <>
      <PageHeader title="Integrations" term="integrations" subtitle="Connect OpenDirectory to the Windows services and cloud apps you already run. Each one has a step-by-step guide and a connection test." />
      <div className="mb-4"><Tabs value={tab} onChange={(t) => setParams({ tab: t })} tabs={[{ id: "windows", label: "Windows Services" }, { id: "cloud", label: "Cloud & Alerts" }]} /></div>
      {mine.length > 0 && (
        <Card className="mb-5" title="Connected" padded={false}>
          <ul className="divide-y divide-line">{mine.map((i) => <ConnectedRow key={i.id} i={i} onEdit={() => setEditing({ def: defs.find((d) => matches(d, i))!, existing: i })} onChanged={list.reload} />)}</ul>
        </Card>
      )}
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
        {defs.map((d) => (
          <div key={d.id} className="flex flex-col rounded-xl border border-line bg-panel p-4">
            <div className="flex items-center gap-2"><span className="grid size-8 place-items-center rounded-lg bg-accent-soft text-accent">{d.icon}</span><span className="font-semibold">{d.title}</span></div>
            <p className="mt-2 flex-1 text-xs leading-relaxed text-ink-2">{d.blurb}</p>
            <div className="mt-3 flex items-center justify-between">
              <Badge tone={d.status === "guide" ? "neutral" : "accent"}>{{ import: "Import users and groups", test: "Guide and connection test", alerts: "Alerts", guide: "Planned" }[d.status]}</Badge>
              <Button size="sm" icon={<Plug className="size-3.5" />} onClick={() => setEditing({ def: d })}>{d.connector ? "Connect" : "Read guide"}</Button>
            </div>
          </div>
        ))}
      </div>
      <div className="mt-5"><Explain>Credentials are stored encrypted on the Management Server and never shown again. Imports are one-way into OpenDirectory and never change the source directory.</Explain></div>
      {editing && <ConnectDialog def={editing.def} existing={editing.existing} onClose={() => setEditing(null)} onSaved={() => { setEditing(null); void list.reload(); }} />}
    </>
  );
}

function matches(d: Def, i: Integration) {
  if (!d.connector || d.connector.type !== i.connector.type) return false;
  return d.connector.type !== "windows_service" || d.connector.service === i.connector.service;
}

function ConnectedRow({ i, onEdit, onChanged }: { i: Integration; onEdit: () => void; onChanged: () => void }) {
  const toast = useToast();
  const [busy, setBusy] = useState<"test" | "import" | null>(null);
  const [confirm, setConfirm] = useState(false);
  return (
    <li className="flex flex-wrap items-center gap-3 px-4 py-3">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2"><span className="font-medium">{i.name}</span>{!i.enabled && <Badge tone="warn">Off</Badge>}</div>
        {i.last_sync_at && <div className={cx("mt-0.5 flex items-center gap-1 text-xs", i.last_ok ? "text-ink-3" : "text-warn")}>{i.last_ok ? <CheckCircle2 className="size-3" /> : <XCircle className="size-3" />}Last import {ago(i.last_sync_at)}: {i.last_message}</div>}
      </div>
      <Button size="sm" loading={busy === "test"} onClick={async () => {
        setBusy("test");
        try { const r = await post<{ ok: boolean; message: string }>(`/integrations/${i.id}/test`); toast(r.ok ? "good" : "bad", r.message); } catch (e) { toast("bad", (e as Error).message); } finally { setBusy(null); }
      }}>Test</Button>
      {i.can_import && <Button size="sm" variant="primary" icon={<RefreshCw className="size-3.5" />} loading={busy === "import"} onClick={async () => {
        setBusy("import");
        try { const r = await post<{ users_created: number; users_updated: number; groups_created: number; note: string }>(`/integrations/${i.id}/import`); toast("good", `${r.users_created} users created, ${r.users_updated} updated, ${r.groups_created} groups created. ${r.note}`); onChanged(); }
        catch (e) { toast("bad", (e as Error).message); onChanged(); } finally { setBusy(null); }
      }}>Import now</Button>}
      <Button size="sm" variant="ghost" icon={<Pencil className="size-3.5" />} onClick={onEdit} aria-label="Edit" />
      <Button size="sm" variant="ghost" icon={<Trash2 className="size-3.5" />} onClick={() => setConfirm(true)} aria-label="Remove" />
      <Confirm open={confirm} onClose={() => setConfirm(false)} danger title={`Remove ${i.name}?`} confirmLabel="Remove" body="Users and groups already imported stay in OpenDirectory."
        onConfirm={async () => { await del(`/integrations/${i.id}`); onChanged(); }} />
    </li>
  );
}

function ConnectDialog({ def, existing, onClose, onSaved }: { def: Def; existing?: Integration; onClose: () => void; onSaved: () => void }) {
  const toast = useToast();
  const [name, setName] = useState(existing?.name ?? def.title);
  const [c, setC] = useState<Conn>(existing?.connector ?? (def.connector ? { ...def.connector } : { type: "" }));
  const [enabled, setEnabled] = useState(existing?.enabled ?? true);
  const [busy, setBusy] = useState(false);
  const save = async () => {
    setBusy(true);
    try { const body = { name, connector: c, enabled }; if (existing) await put(`/integrations/${existing.id}`, body); else await post("/integrations", body); toast("good", `${name} saved. Use Test to check it.`); onSaved(); }
    catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
  };
  return (
    <Dialog open onClose={onClose} wide title={<span className="flex items-center gap-2">{def.icon}{def.title}</span>}
      footer={def.connector ? <><Button onClick={onClose}>Cancel</Button><Button variant="primary" loading={busy} disabled={!name.trim()} onClick={save}>{existing ? "Save" : "Connect"}</Button></> : <Button onClick={onClose}>Close</Button>}>
      <div className="grid max-h-[65vh] gap-5 overflow-y-auto md:grid-cols-2">
        <div>
          <h4 className="mb-2 text-sm font-semibold">Step by step</h4>
          <ol className="space-y-2.5 text-sm text-ink-2">
            {def.steps.map((s, i) => <li key={i} className="flex gap-2.5"><span className="grid size-5 shrink-0 place-items-center rounded-full bg-accent-soft text-[11px] font-semibold text-accent">{i + 1}</span><span className="leading-relaxed">{s}</span></li>)}
          </ol>
          {def.note && <div className="mt-3"><Callout tone="info">{def.note}</Callout></div>}
        </div>
        {def.connector && def.fields && (
          <div className="space-y-3">
            <Field label="Name"><Input value={name} onChange={(e) => setName(e.target.value)} /></Field>
            {def.fields.map(([k, label, kind, ph]) => {
              const v = c[k];
              if (kind === "bool") return <Switch key={k} checked={!!v} onChange={(x) => setC({ ...c, [k]: x })} label={label} />;
              if (kind === "list") return <Field key={k} label={label} hint={ph}><Input value={((v as string[]) ?? []).join(", ")} onChange={(e) => setC({ ...c, [k]: e.target.value.split(",").map((x) => x.trim()).filter(Boolean) })} /></Field>;
              return <Field key={k} label={label}><Input type={kind === "secret" ? "password" : kind === "number" ? "number" : "text"} placeholder={ph} value={String(v ?? "")}
                onFocus={() => { if (v === KEEP) setC({ ...c, [k]: "" }); }} onChange={(e) => setC({ ...c, [k]: kind === "number" ? Number(e.target.value) : e.target.value })} /></Field>;
            })}
            {def.status === "alerts" && <Switch checked={enabled} onChange={setEnabled} label="Send events" />}
            {existing && <p className="text-xs text-ink-3">Leave secret fields unchanged to keep the saved values.</p>}
          </div>
        )}
      </div>
    </Dialog>
  );
}
