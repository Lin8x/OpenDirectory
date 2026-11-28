import { useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { ArrowLeft, Cpu, HardDrive, MemoryStick, MessageSquare, Network, Play, RefreshCw, RotateCw, ShieldCheck, Trash2, UserRound } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { del, get, patch, post } from "../lib/api";
import type { Endpoint, Job, OrgUnit, RsopEntry, Shell } from "../lib/types";
import { ago, bytes, osLabel, uptime } from "../lib/format";
import { Badge, Button, Card, Confirm, CopyButton, ErrorNote, Explain, Field, Loading, OsIcon, Select, StatusDot, Tabs, Textarea, complianceLabel, complianceTone, useToast, GuideLink } from "../components/ui";
import { OuSelect } from "./Users";
import { JobRow } from "./Jobs";
import { RemoteShell } from "../components/RemoteShell";
import { RemoteDesktop } from "../components/RemoteDesktop";
import { RemoteViewer } from "../components/RemoteViewer";
import { HelpDeskChat } from "../components/HelpDeskChat";
import { ManageTab } from "../components/manage/Manage";
import { Dialog, Input } from "../components/ui";

export default function EndpointDetail() {
  const { id } = useParams();
  const nav = useNavigate();
  const toast = useToast();
  const ep = useQuery<Endpoint>(`/endpoints/${id}`, { poll: 8000 });
  const ous = useQuery<OrgUnit[]>("/ous");
  const [tab, setTab] = useState<"overview" | "policy" | "manage" | "scripts" | "shell" | "helpdesk">("overview");
  const [confirm, setConfirm] = useState(false);
  const [notify, setNotify] = useState<{ title: string; message: string } | null>(null);

  if (ep.loading && !ep.data) return <Loading />;
  if (ep.error && !ep.data) return <ErrorNote message={ep.error} onRetry={ep.reload} />;
  const e = ep.data!;
  const i = e.inventory;
  const action = async (type: "refresh_inventory" | "restart_agent", msg: string) => {
    try { await post(`/endpoints/${e.id}/jobs`, { type }); toast("good", msg); } catch (err) { toast("bad", (err as Error).message); }
  };

  return (
    <div className="space-y-5">
      <Link to="/endpoints" className="inline-flex items-center gap-1 text-sm text-ink-3 hover:text-ink"><ArrowLeft className="size-4" />Endpoints</Link>
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="flex items-center gap-4">
          <div className="grid size-14 place-items-center rounded-2xl border border-line bg-panel"><OsIcon os={e.os} className="size-7" /></div>
          <div>
            <h1 className="flex items-center gap-2.5 text-[22px] font-semibold tracking-tight">{e.hostname}<StatusDot online={e.online} /><GuideLink to="04-endpoints-and-agents" className="font-normal" /></h1>
            <p className="text-sm text-ink-2">{i.os_name} {i.os_edition && <span className="text-ink-3">· {i.os_edition}</span>} · {e.online ? <span className="text-good">Online</span> : `Last seen ${ago(e.last_seen_at)}`}</p>
          </div>
        </div>
        <div className="flex gap-2">
          <Button icon={<RefreshCw className="size-4" />} onClick={() => action("refresh_inventory", "Inventory refresh requested")}>Refresh inventory</Button>
          <Button icon={<MessageSquare className="size-4" />} onClick={() => setNotify({ title: "Message from IT", message: "" })}>Message user</Button>
          {e.os !== "android" && <Button icon={<RotateCw className="size-4" />} onClick={() => action("restart_agent", "Agent restart requested")}>Restart agent</Button>}
          <Button variant="ghost" icon={<Trash2 className="size-4" />} onClick={() => setConfirm(true)}>Retire</Button>
        </div>
      </div>

      <Tabs value={tab} onChange={setTab} tabs={[
        { id: "overview", label: "Overview" }, { id: "policy", label: "Policy & compliance" },
        ...(e.os === "android" ? [] : [{ id: "manage" as const, label: "Manage" }, { id: "scripts" as const, label: "Remote scripts" }, { id: "shell" as const, label: "Remote shell & desktop" }]),
        { id: "helpdesk", label: "Help desk" },
      ]} />
      {tab === "helpdesk" && <div className="rounded-xl border border-line bg-panel"><HelpDeskChat endpointId={e.id} hostname={e.hostname} /></div>}
      <Dialog open={!!notify} onClose={() => setNotify(null)} title={`Message the user at ${e.hostname}`}
        footer={<><Button onClick={() => setNotify(null)}>Cancel</Button><Button variant="primary" disabled={!notify?.title.trim() || !notify?.message.trim()}
          onClick={async () => { try { await post(`/endpoints/${e.id}/jobs`, { type: "notify_user", ...notify }); toast("good", "Message sent. The job result shows whether someone saw it."); setNotify(null); } catch (err) { toast("bad", (err as Error).message); } }}>Send</Button></>}>
        {notify && <div className="space-y-3">
          <Field label="Title"><Input value={notify.title} maxLength={120} onChange={(ev) => setNotify({ ...notify, title: ev.target.value })} /></Field>
          <Field label="Message"><Textarea rows={4} value={notify.message} maxLength={2000} onChange={(ev) => setNotify({ ...notify, message: ev.target.value })} placeholder="We'll restart this computer at 6 PM for updates." /></Field>
          <p className="text-xs text-ink-3">Pops up on the screen of whoever is signed in. For a conversation, use the Help desk tab.</p>
        </div>}
      </Dialog>

      {tab === "overview" && <LapsCard id={e.id} />}
      {tab === "overview" && (
        <div className="grid gap-5 lg:grid-cols-3">
          <Card title="Hardware" className="lg:col-span-2">
            <div className="grid gap-x-8 gap-y-4 sm:grid-cols-2">
              <Spec label="Manufacturer" value={i.manufacturer} />
              <Spec label="Model" value={i.model} />
              <Spec label="Serial number" value={<span className="selectable font-mono">{i.serial_number || "—"}</span>} />
              <Spec label="Architecture" value={i.arch} />
              <Spec icon={<Cpu className="size-3.5" />} label="Processor" value={`${i.cpu_model} (${i.cpu_cores} threads)`} />
              <Spec icon={<MemoryStick className="size-3.5" />} label="Memory" value={bytes(i.memory_bytes)} />
              <Spec label="OS version" value={`${osLabel[e.os]} ${i.os_version}`} />
              <Spec label="Kernel" value={i.kernel_version} />
              <Spec label="Uptime" value={uptime(i.uptime_secs)} />
              <Spec label="Agent version" value={e.agent_version} />
            </div>
            <h4 className="mb-2 mt-6 flex items-center gap-1.5 text-xs font-semibold text-ink-3"><HardDrive className="size-3.5" />Storage</h4>
            <div className="space-y-2.5">
              {i.disks.map((d) => {
                const used = d.total_bytes - d.free_bytes; const pct = d.total_bytes ? used / d.total_bytes : 0;
                return (
                  <div key={d.mount} className="text-sm">
                    <div className="mb-1 flex justify-between"><span className="font-mono text-[12.5px]">{d.mount} <span className="text-ink-3">{d.file_system}</span></span><span className="text-ink-3">{bytes(d.free_bytes)} free of {bytes(d.total_bytes)}</span></div>
                    <div className="h-1.5 overflow-hidden rounded-full bg-panel-2"><div className={`h-full rounded-full ${pct > 0.9 ? "bg-bad" : pct > 0.75 ? "bg-warn" : "bg-accent"}`} style={{ width: `${pct * 100}%` }} /></div>
                  </div>
                );
              })}
            </div>
          </Card>
          <div className="space-y-5">
            <Card title="Security">
              <div className="space-y-2.5 text-sm">
                <SecRow label="Disk encryption" v={i.disk_encrypted} />
                <SecRow label="Firewall" v={i.firewall_enabled} />
              </div>
            </Card>
            <Card title={<span className="flex items-center gap-1.5"><UserRound className="size-4" />Signed-in users</span>}>
              {i.logged_in_users.length ? <div className="flex flex-wrap gap-1.5">{i.logged_in_users.map((u) => <Badge key={u}>{u}</Badge>)}</div> : <p className="text-sm text-ink-3">Nobody is signed in.</p>}
            </Card>
            <Card title={<span className="flex items-center gap-1.5"><Network className="size-4" />Network</span>}>
              <div className="space-y-2.5 text-sm">
                {i.interfaces.map((n) => (
                  <div key={n.name}><div className="font-medium">{n.name}</div><div className="selectable font-mono text-xs text-ink-3">{n.mac}</div>{n.ips.map((ip) => <div key={ip} className="selectable font-mono text-xs">{ip}</div>)}</div>
                ))}
                <div className="border-t border-line pt-2 text-xs text-ink-3">Seen from <span className="font-mono">{e.public_ip}</span></div>
              </div>
            </Card>
            <Placement e={e} ous={ous.data ?? []} onSaved={ep.reload} />
          </div>
        </div>
      )}
      {tab === "policy" && <PolicyTab e={e} />}
      {tab === "manage" && <ManageTab endpoint={e} />}
      {tab === "scripts" && <ScriptsTab e={e} />}
      {tab === "shell" && <div className="space-y-5"><RemoteViewer endpoint={e} /><RemoteShell endpoint={e} /><RemoteDesktop endpoint={e} /></div>}

      <Confirm open={confirm} onClose={() => setConfirm(false)} danger title={`Retire ${e.hostname}?`} confirmLabel="Retire endpoint"
        body="The agent's credentials are revoked and the endpoint is removed from the directory. Settings and accounts already applied stay on the computer. To manage it again, enroll it with a new token."
        onConfirm={async () => { await del(`/endpoints/${e.id}`); toast("good", "Endpoint retired"); nav("/endpoints"); }} />
    </div>
  );
}

function Spec({ label, value, icon }: { label: string; value: React.ReactNode; icon?: React.ReactNode }) {
  return <div><div className="flex items-center gap-1 text-xs text-ink-3">{icon}{label}</div><div className="mt-0.5 text-sm">{value || "—"}</div></div>;
}
function SecRow({ label, v }: { label: string; v: boolean | null }) {
  return <div className="flex items-center justify-between"><span className="text-ink-2">{label}</span>{v === null ? <Badge>Unknown</Badge> : v ? <Badge tone="good">On</Badge> : <Badge tone="warn">Off</Badge>}</div>;
}

function Placement({ e, ous, onSaved }: { e: Endpoint; ous: OrgUnit[]; onSaved: () => void }) {
  const toast = useToast();
  const [ou, setOu] = useState(e.ou_id);
  const [notes, setNotes] = useState(e.notes);
  useEffect(() => { setOu(e.ou_id); setNotes(e.notes); }, [e.ou_id, e.notes]);
  const dirty = ou !== e.ou_id || notes !== e.notes;
  return (
    <Card title="Placement">
      <div className="space-y-3">
        <Field label="Organizational unit" term="ou" hint="GPOs linked to this OU apply here."><OuSelect ous={ous} value={ou} onChange={setOu} /></Field>
        <Field label="Notes"><Textarea rows={2} value={notes} onChange={(ev) => setNotes(ev.target.value)} placeholder="Asset tag, location, owner…" /></Field>
        <div className="flex justify-end"><Button size="sm" variant="primary" disabled={!dirty} onClick={async () => {
          try { await patch(`/endpoints/${e.id}`, { ou_id: ou, notes }); toast("good", "Saved"); onSaved(); } catch (err) { toast("bad", (err as Error).message); }
        }}>Save</Button></div>
      </div>
    </Card>
  );
}

function PolicyTab({ e }: { e: Endpoint }) {
  const rsop = useQuery<RsopEntry[]>(`/endpoints/${e.id}/rsop`, { poll: 10000 });
  return (
    <div className="space-y-4">
      <Explain term="rsop" />
      <Card title="Resultant Set of Policy" subtitle="Every setting that applies to this endpoint, the GPO it comes from, and what the agent last reported" padded={false}>
        {rsop.loading && !rsop.data ? <Loading /> : (rsop.data ?? []).length === 0 ? (
          <div className="p-6 text-center text-sm text-ink-3">No GPOs apply to this endpoint. <Link to="/gpos/new" className="text-accent hover:underline">Create one</Link>.</div>
        ) : (
          <ul className="divide-y divide-line">
            {rsop.data!.map((r, idx) => (
              <li key={idx} className="flex flex-wrap items-start gap-4 px-4 py-3">
                <ShieldCheck className="mt-0.5 size-4 text-accent" />
                <div className="min-w-0 flex-1">
                  <div className="text-sm font-medium">{r.description}</div>
                  <div className="mt-0.5 text-xs text-ink-3">
                    From <Link to={`/gpos/${r.policy_id}`} className="text-accent hover:underline">{r.policy_name}</Link>
                    {r.overridden.length > 0 && <> · overrides {r.overridden.join(", ")}</>}
                    {r.support && <> · {r.support.mechanism}</>}
                  </div>
                  {r.compliance?.message && <div className="mt-1 text-xs text-ink-2">{r.compliance.message}</div>}
                </div>
                {r.compliance ? <Badge tone={complianceTone[r.compliance.state]}>{complianceLabel[r.compliance.state]}</Badge> : <Badge>Pending</Badge>}
              </li>
            ))}
          </ul>
        )}
      </Card>
      <Explain term="compliance" />
    </div>
  );
}

const SNIPPETS: Record<string, { label: string; shell: Shell; script: string }[]> = {
  windows: [
    { label: "Installed updates", shell: "powershell", script: "Get-HotFix | Sort-Object InstalledOn -Descending | Select-Object -First 10" },
    { label: "Disk usage", shell: "powershell", script: "Get-PSDrive -PSProvider FileSystem | Format-Table Name, Used, Free" },
    { label: "Local administrators", shell: "powershell", script: "Get-LocalGroupMember -SID S-1-5-32-544" },
    { label: "Flush DNS", shell: "cmd", script: "ipconfig /flushdns" },
  ],
  macos: [
    { label: "Software updates", shell: "zsh", script: "softwareupdate --list" },
    { label: "Disk usage", shell: "zsh", script: "df -h /" },
    { label: "FileVault status", shell: "zsh", script: "fdesetup status" },
    { label: "Local admins", shell: "zsh", script: "dscl . -read /Groups/admin GroupMembership" },
  ],
  linux: [
    { label: "Pending updates", shell: "bash", script: "(apt list --upgradable 2>/dev/null || dnf check-update) | head -30" },
    { label: "Disk usage", shell: "bash", script: "df -h" },
    { label: "Failed services", shell: "bash", script: "systemctl --failed" },
    { label: "Who is logged in", shell: "bash", script: "who; last -n 5" },
  ],
};

function ScriptsTab({ e }: { e: Endpoint }) {
  const toast = useToast();
  const jobs = useQuery<Job[]>(`/endpoints/${e.id}/jobs`, { poll: 3000 });
  const defShell: Shell = e.os === "windows" ? "powershell" : e.os === "macos" ? "zsh" : "bash";
  const [shell, setShell] = useState<Shell>(defShell);
  const [script, setScript] = useState("");
  const [busy, setBusy] = useState(false);
  const [lastRun, setLastRun] = useState<string | null>(null);
  const run = async () => {
    setBusy(true);
    try {
      const j = await post<Job>(`/endpoints/${e.id}/jobs`, { type: "script", shell, script });
      setLastRun(j.id);
      toast("good", e.online ? "Script sent. Output appears in History in a few seconds." : "Script queued. It runs when the endpoint reconnects.");
      void jobs.reload();
    }
    catch (err) { toast("bad", (err as Error).message); } finally { setBusy(false); }
  };
  return (
    <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)]">
      <Card title="Run a script" subtitle={`Runs as ${e.os === "windows" ? "SYSTEM" : "root"} on ${e.hostname}`}>
        <div className="space-y-3">
          <div className="flex flex-wrap gap-1.5">
            {(SNIPPETS[e.os] ?? []).map((s) => <button key={s.label} onClick={() => { setShell(s.shell); setScript(s.script); }} className="rounded-full border border-line px-2.5 py-1 text-xs text-ink-2 hover:border-accent hover:text-accent">{s.label}</button>)}
          </div>
          <Field label="Shell">
            <Select value={shell} onChange={(ev) => setShell(ev.target.value as Shell)}>
              {e.os === "windows" ? <><option value="powershell">PowerShell</option><option value="cmd">Command Prompt</option></> : <><option value="bash">bash</option><option value="zsh">zsh</option><option value="default">sh</option><option value="powershell">PowerShell (pwsh)</option></>}
            </Select>
          </Field>
          <Textarea rows={9} className="font-mono text-[12.5px]" value={script} onChange={(ev) => setScript(ev.target.value)} placeholder={e.os === "windows" ? "Get-Service | Where-Object Status -eq 'Running'" : "uptime"} spellCheck={false} />
          <div className="flex items-center justify-between">
            <span className="text-xs text-ink-3">{e.online ? "Endpoint is online; expect results in seconds." : "Endpoint is offline; the script runs when it reconnects."}</span>
            <Button variant="primary" icon={<Play className="size-4" />} loading={busy} disabled={!script.trim()} onClick={run}>Run</Button>
          </div>
        </div>
      </Card>
      <Card title="History" padded={false}>
        {(jobs.data ?? []).length === 0 ? <p className="p-6 text-center text-sm text-ink-3">No scripts have run on this endpoint yet.</p> : (
          <ul className="divide-y divide-line">{jobs.data!.map((j) => <JobRow key={j.id} job={j} onChanged={jobs.reload} defaultOpen={j.id === lastRun} />)}</ul>
        )}
      </Card>
    </div>
  );
}

export function LapsCard({ id }: { id: string }) {
  const toast = useToast();
  const [v, setV] = useState<{ account: string; password: string; previous_password: string | null; rotated_at: string; rotation_pending: boolean } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  return (
    <div className="mb-5 flex flex-wrap items-center gap-3 rounded-xl border border-line bg-panel px-4 py-3">
      <span className="text-sm font-semibold">Local admin password (LAPS)</span>
      {v ? (
        <>
          <span className="text-sm text-ink-2">{v.account}</span>
          <code className="selectable rounded bg-panel-2 px-2 py-0.5 font-mono text-sm">{v.password}</code>
          <CopyButton text={v.password} />
          <span className="text-xs text-ink-3">rotated {ago(v.rotated_at)}{v.rotation_pending ? " · rotation requested" : ""}</span>
        </>
      ) : err ? <span className="text-xs text-ink-3">{err}</span> : <span className="text-xs text-ink-3">Hidden. Viewing it is recorded in the audit log.</span>}
      <div className="ml-auto flex gap-2">
        {!v && <Button size="sm" onClick={async () => { try { setV(await get(`/endpoints/${id}/laps`)); setErr(null); } catch (e) { setErr((e as Error).message); } }}>Show password</Button>}
        <Button size="sm" variant="ghost" onClick={async () => { try { const r = await post<{ message: string }>(`/endpoints/${id}/laps/rotate`); toast("good", r.message); } catch (e) { toast("bad", (e as Error).message); } }}>Rotate now</Button>
      </div>
    </div>
  );
}
