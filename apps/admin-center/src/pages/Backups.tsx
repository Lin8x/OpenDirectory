// Backup & Restore: one-file encrypted backups of the whole configuration,
// on a schedule to any of many destinations, plus restore.
import { useEffect, useRef, useState } from "react";
import { AlertTriangle, CheckCircle2, CloudUpload, DatabaseBackup, Download, HardDrive, Pencil, Play, Plus, RotateCcw, Trash2, Upload, XCircle } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { del, post, put, saveBinary } from "../lib/api";
import { ago, dateTime } from "../lib/format";
import { Badge, Button, Callout, Card, Confirm, Dialog, EmptyState, Explain, Field, Input, Loading, PageHeader, Select, Switch, cx, useToast } from "../components/ui";

const KEEP = "••••••••";
type DestType = "local" | "sftp" | "ftp" | "webdav" | "s3" | "http" | "google_drive" | "onedrive" | "dropbox";
type Dest = { type: DestType } & Record<string, string | number | boolean>;
interface Plan {
  id: string; name: string; kind: string; destination: Dest; schedule: { frequency: "manual" | "hourly" | "daily" | "weekly"; time: string; weekday: number };
  retention: { keep_last: number; max_age_days: number }; encrypt: boolean; has_passphrase: boolean; enabled: boolean;
  last_run_at: string | null; last_ok: boolean | null; last_message: string; next_run_at: string | null; warnings: string[];
}
interface RemoteFile { name: string; size: number; modified: string | null }
interface Verified { header: { created_at: string; domain: string | null; organization: string | null; encrypted: boolean; server_version: string }; users: number; groups: number; endpoints: number; policies: number }

// Field definitions per destination: [key, label, kind, placeholder/hint]
type F = [string, string, "text" | "secret" | "number" | "bool" | "textarea", string?];
const DESTS: { type: DestType; label: string; blurb: string; fields: F[]; defaults: Record<string, string | number | boolean>; help?: string }[] = [
  { type: "local", label: "Folder or network share", blurb: "A folder on the server, or a mounted SMB/NFS share. On Windows a UNC path like \\\\nas\\backups works directly.", fields: [["path", "Folder", "text", "/mnt/nas/opendirectory or \\\\nas\\backups"]], defaults: { path: "" } },
  { type: "sftp", label: "SFTP", blurb: "Any SSH server. The host key is pinned on first connect.", fields: [["host", "Host", "text", "backup.example.com"], ["port", "Port", "number"], ["username", "Username", "text"], ["password", "Password", "secret", "Or use a private key"], ["private_key", "Private key (OpenSSH)", "textarea", "-----BEGIN OPENSSH PRIVATE KEY-----"], ["path", "Folder", "text", "backups/opendirectory"], ["host_key", "Pinned host key", "text", "Learned on first connect"]], defaults: { host: "", port: 22, username: "", password: "", private_key: "", path: "", host_key: "" } },
  { type: "ftp", label: "FTP / FTPS", blurb: "Explicit FTPS (AUTH TLS) by default. Plain FTP is allowed but sends the password unencrypted.", fields: [["host", "Host", "text"], ["port", "Port", "number"], ["username", "Username", "text"], ["password", "Password", "secret"], ["path", "Folder", "text"], ["tls", "Use TLS (FTPS)", "bool"]], defaults: { host: "", port: 21, username: "", password: "", path: "", tls: true } },
  { type: "webdav", label: "WebDAV", blurb: "Nextcloud, ownCloud, Synology, Box and other WebDAV servers.", fields: [["url", "Folder URL", "text", "https://cloud.example.com/remote.php/dav/files/me/Backups"], ["username", "Username", "text"], ["password", "Password or app password", "secret"]], defaults: { url: "", username: "", password: "" } },
  { type: "s3", label: "S3-compatible", blurb: "Amazon S3, Cloudflare R2, Backblaze B2, Wasabi, MinIO and others.", fields: [["endpoint", "Endpoint", "text", "https://s3.us-east-1.amazonaws.com"], ["region", "Region", "text", "us-east-1"], ["bucket", "Bucket", "text"], ["prefix", "Folder prefix", "text", "opendirectory"], ["access_key", "Access key ID", "text"], ["secret_key", "Secret access key", "secret"], ["path_style", "Path-style URLs (MinIO, most self-hosted)", "bool"]], defaults: { endpoint: "", region: "us-east-1", bucket: "", prefix: "opendirectory", access_key: "", secret_key: "", path_style: false } },
  { type: "http", label: "Custom HTTPS endpoint", blurb: "Any server and port that accepts an HTTPS PUT. Retention isn't possible because it can't list files.", fields: [["url", "Base URL", "text", "https://backup.example.com:9443/upload"], ["auth_header", "Auth header name", "text", "Authorization"], ["auth_value", "Auth header value", "secret", "Bearer …"]], defaults: { url: "", auth_header: "", auth_value: "" } },
  { type: "google_drive", label: "Google Drive", blurb: "Uses an OAuth client and refresh token from your Google Cloud project.", fields: [["client_id", "OAuth client ID", "text"], ["client_secret", "Client secret", "secret"], ["refresh_token", "Refresh token", "secret"], ["folder_id", "Folder ID", "text", "From the folder's URL"]], defaults: { client_id: "", client_secret: "", refresh_token: "", folder_id: "" }, help: "Create an OAuth client (Desktop app) in Google Cloud Console with the Drive API enabled, then get a refresh token with the drive.file scope, for example with Google's OAuth Playground." },
  { type: "onedrive", label: "OneDrive", blurb: "Personal or work OneDrive through a Microsoft Entra app registration.", fields: [["client_id", "Application (client) ID", "text"], ["client_secret", "Client secret (optional)", "secret"], ["refresh_token", "Refresh token", "secret"], ["folder", "Folder", "text"]], defaults: { client_id: "", client_secret: "", refresh_token: "", folder: "OpenDirectory Backups" }, help: "Register an app in Microsoft Entra with the Files.ReadWrite and offline_access delegated permissions, sign in once to get a refresh token, and paste it here." },
  { type: "dropbox", label: "Dropbox", blurb: "Uses a Dropbox app with a refresh token.", fields: [["app_key", "App key", "text"], ["app_secret", "App secret", "secret"], ["refresh_token", "Refresh token", "secret"], ["folder", "Folder", "text"]], defaults: { app_key: "", app_secret: "", refresh_token: "", folder: "/OpenDirectory Backups" }, help: "Create a Dropbox app (App folder access) at dropbox.com/developers, then generate a refresh token with token_access_type=offline." },
];
const DEST_ICON: Partial<Record<DestType, React.ReactNode>> = { local: <HardDrive className="size-4" /> };

export default function Backups() {
  const plans = useQuery<Plan[]>("/backups/plans", { poll: 15000 });
  const [editing, setEditing] = useState<Plan | "new" | null>(null);
  const [restoring, setRestoring] = useState<Plan | "upload" | null>(null);
  const [downloading, setDownloading] = useState(false);
  return (
    <>
      <PageHeader title="Backup & Restore" term="backup"
        subtitle="Back up the whole configuration (directory, GPOs, scripts, keys and certificate) to one encrypted file, on a schedule, to the destinations you choose."
        actions={<div className="flex gap-2"><Button icon={<Download className="size-4" />} onClick={() => setDownloading(true)}>Download a backup now</Button><Button variant="primary" icon={<Plus className="size-4" />} onClick={() => setEditing("new")}>New backup plan</Button></div>} />
      {plans.loading && !plans.data ? <Loading /> : (plans.data ?? []).length === 0 ? (
        <Card><EmptyState icon={<DatabaseBackup className="size-5" />} title="No backup plans yet" body="A plan backs up on a schedule to a folder, SFTP, FTPS, WebDAV, S3, Google Drive, OneDrive, Dropbox or any HTTPS endpoint, and removes old backups for you."
          action={<Button variant="primary" onClick={() => setEditing("new")}>Create a backup plan</Button>} /></Card>
      ) : (
        <div className="grid gap-4 lg:grid-cols-2">
          {plans.data!.map((p) => <PlanCard key={p.id} plan={p} onEdit={() => setEditing(p)} onRestore={() => setRestoring(p)} onChanged={plans.reload} />)}
        </div>
      )}
      <Card className="mt-5" title="Restore">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <p className="max-w-2xl text-sm text-ink-2">Restore from a <code className="font-mono">.odbackup</code> file, or choose a backup inside a plan above. The restore is applied when the Management Server restarts, and the current data is kept in a <code className="font-mono">pre-restore</code> folder.</p>
          <Button icon={<Upload className="size-4" />} onClick={() => setRestoring("upload")}>Restore from a file…</Button>
        </div>
      </Card>
      <div className="mt-5"><Explain>Backups contain the server's master key, which can decrypt every user's password. Keep them encrypted, and store the passphrase somewhere safe: without it, an encrypted backup can't be restored. Destination passwords and tokens are stored encrypted on the server and are never shown again.</Explain></div>
      {editing && <PlanDialog plan={editing === "new" ? null : editing} onClose={() => setEditing(null)} onSaved={() => { setEditing(null); void plans.reload(); }} />}
      {restoring && <RestoreDialog plan={restoring === "upload" ? null : restoring} onClose={() => setRestoring(null)} />}
      {downloading && <DownloadDialog onClose={() => setDownloading(false)} />}
    </>
  );
}

function PlanCard({ plan: p, onEdit, onRestore, onChanged }: { plan: Plan; onEdit: () => void; onRestore: () => void; onChanged: () => void }) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const sched = p.schedule.frequency === "manual" ? "Manual only" : p.schedule.frequency === "hourly" ? "Every hour" :
    p.schedule.frequency === "daily" ? `Daily at ${p.schedule.time}` : `Weekly, ${["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][p.schedule.weekday]} at ${p.schedule.time}`;
  return (
    <Card title={<span className="flex items-center gap-2">{DEST_ICON[p.destination.type] ?? <CloudUpload className="size-4" />}{p.name}</span>}
      actions={<div className="flex gap-1"><Button size="sm" variant="ghost" icon={<Pencil className="size-3.5" />} onClick={onEdit}>Edit</Button><Button size="sm" variant="ghost" icon={<Trash2 className="size-3.5" />} onClick={() => setConfirm(true)} aria-label="Delete" /></div>}>
      <div className="space-y-3 text-sm">
        <div className="flex flex-wrap gap-1.5">
          <Badge>{p.kind}</Badge><Badge>{sched}</Badge>
          {p.encrypt ? <Badge tone="good">Encrypted</Badge> : <Badge tone="warn">Not encrypted</Badge>}
          <Badge>{p.retention.keep_last ? `Keep ${p.retention.keep_last}` : "Keep all"}{p.retention.max_age_days ? `, max ${p.retention.max_age_days} d` : ""}</Badge>
          {!p.enabled && <Badge tone="warn">Paused</Badge>}
        </div>
        {p.last_run_at ? (
          <div className={cx("flex gap-2 rounded-lg p-2.5 text-xs", p.last_ok ? "bg-good-soft" : "bg-warn-soft")}>
            {p.last_ok ? <CheckCircle2 className="mt-0.5 size-3.5 shrink-0 text-good" /> : <XCircle className="mt-0.5 size-3.5 shrink-0 text-warn" />}
            <div><div className="font-medium">{p.last_ok ? "Last backup succeeded" : "Last backup failed"} · {ago(p.last_run_at)}</div><div className="selectable mt-0.5 break-all text-ink-2">{p.last_message}</div></div>
          </div>
        ) : <p className="text-xs text-ink-3">Hasn't run yet.</p>}
        {p.next_run_at && <p className="text-xs text-ink-3">Next backup: {dateTime(p.next_run_at)}</p>}
        {p.warnings.map((w) => <p key={w} className="flex gap-1.5 text-xs text-warn"><AlertTriangle className="mt-0.5 size-3.5 shrink-0" />{w}</p>)}
        <div className="flex flex-wrap gap-2 pt-1">
          <Button size="sm" variant="primary" icon={<Play className="size-3.5" />} loading={busy} onClick={async () => {
            setBusy(true);
            try { const r = await post<{ ok: boolean; message: string }>(`/backups/plans/${p.id}/run`); toast(r.ok ? "good" : "bad", r.message); onChanged(); } catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
          }}>Back up now</Button>
          <Button size="sm" icon={<RotateCcw className="size-3.5" />} onClick={onRestore}>Browse and restore</Button>
        </div>
      </div>
      <Confirm open={confirm} onClose={() => setConfirm(false)} danger title={`Delete ${p.name}?`} confirmLabel="Delete plan" body="Scheduled backups stop. Backups already saved at the destination are kept."
        onConfirm={async () => { await del(`/backups/plans/${p.id}`); toast("good", "Backup plan deleted"); onChanged(); }} />
    </Card>
  );
}

function PlanDialog({ plan, onClose, onSaved }: { plan: Plan | null; onClose: () => void; onSaved: () => void }) {
  const toast = useToast();
  const [name, setName] = useState(plan?.name ?? "Nightly backup");
  const [dest, setDest] = useState<Dest>(plan?.destination ?? { type: "local", path: "" });
  const [schedule, setSchedule] = useState(plan?.schedule ?? { frequency: "daily" as const, time: "02:00", weekday: 6 });
  const [retention, setRetention] = useState(plan?.retention ?? { keep_last: 14, max_age_days: 0 });
  const [encrypt, setEncrypt] = useState(plan?.encrypt ?? true);
  const [pass, setPass] = useState(plan?.has_passphrase ? KEEP : "");
  const [pass2, setPass2] = useState(plan?.has_passphrase ? KEEP : "");
  const [enabled, setEnabled] = useState(plan?.enabled ?? true);
  const [busy, setBusy] = useState(false);
  const [test, setTest] = useState<{ ok: boolean; message: string } | null>(null);
  const [testing, setTesting] = useState(false);
  const def = DESTS.find((d) => d.type === dest.type)!;
  const passOk = !encrypt || (pass === KEEP && pass2 === KEEP) || (pass.length >= 12 && pass === pass2);
  const setField = (k: string, v: string | number | boolean) => { setDest({ ...dest, [k]: v }); setTest(null); };
  const save = async () => {
    setBusy(true);
    const body = { name, destination: dest, schedule, retention, encrypt, passphrase: pass === KEEP ? "" : pass, enabled };
    try { if (plan) await put(`/backups/plans/${plan.id}`, body); else await post("/backups/plans", body); toast("good", "Backup plan saved"); onSaved(); }
    catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
  };
  return (
    <Dialog open onClose={onClose} wide title={plan ? `Edit ${plan.name}` : "New backup plan"}
      footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" loading={busy} disabled={!name.trim() || !passOk} onClick={save}>Save plan</Button></>}>
      <div className="max-h-[65vh] space-y-4 overflow-y-auto pr-1">
        <Field label="Name"><Input value={name} onChange={(e) => setName(e.target.value)} /></Field>
        <Field label="Destination">
          <div className="grid gap-1.5 sm:grid-cols-3">
            {DESTS.map((d) => (
              <button key={d.type} onClick={() => { setDest({ type: d.type, ...d.defaults }); setTest(null); }}
                className={cx("rounded-lg border px-2.5 py-2 text-left text-xs", dest.type === d.type ? "border-accent bg-accent-soft/50 font-semibold" : "border-line hover:border-accent/60")}>{d.label}</button>
            ))}
          </div>
        </Field>
        <p className="text-xs text-ink-3">{def.blurb}</p>
        {def.help && <Callout tone="info" title="Before you start">{def.help}</Callout>}
        <div className="grid gap-3 sm:grid-cols-2">
          {def.fields.map(([k, label, kind, ph]) => {
            const v = dest[k] ?? "";
            if (kind === "bool") return <div key={k} className="sm:col-span-2"><Switch checked={!!v} onChange={(x) => setField(k, x)} label={label} /></div>;
            if (kind === "textarea") return <div key={k} className="sm:col-span-2"><Field label={label}><textarea rows={3} className="w-full rounded-md border border-line bg-panel px-3 py-2 font-mono text-xs" placeholder={ph} value={String(v)} onChange={(e) => setField(k, e.target.value)} /></Field></div>;
            return <Field key={k} label={label}><Input type={kind === "secret" ? "password" : kind === "number" ? "number" : "text"} placeholder={ph} value={String(v)} onChange={(e) => setField(k, kind === "number" ? Number(e.target.value) : e.target.value)} /></Field>;
          })}
        </div>
        <div className="flex items-center gap-3">
          <Button size="sm" loading={testing} onClick={async () => {
            setTesting(true);
            try { setTest(await post(plan ? `/backups/plans/${plan.id}/test` : "/backups/test", { destination: dest })); } catch (e) { setTest({ ok: false, message: (e as Error).message }); } finally { setTesting(false); }
          }}>Test connection</Button>
          {test && <span className={cx("text-xs", test.ok ? "text-good" : "text-bad")}>{test.message}</span>}
        </div>
        <div className="grid gap-3 sm:grid-cols-3">
          <Field label="Schedule"><Select value={schedule.frequency} onChange={(e) => setSchedule({ ...schedule, frequency: e.target.value as Plan["schedule"]["frequency"] })}><option value="manual">Manual only</option><option value="hourly">Every hour</option><option value="daily">Daily</option><option value="weekly">Weekly</option></Select></Field>
          {(schedule.frequency === "daily" || schedule.frequency === "weekly") && <Field label="Time (server's local time)"><Input type="time" value={schedule.time} onChange={(e) => setSchedule({ ...schedule, time: e.target.value })} /></Field>}
          {schedule.frequency === "weekly" && <Field label="Day"><Select value={schedule.weekday} onChange={(e) => setSchedule({ ...schedule, weekday: Number(e.target.value) })}>{["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"].map((d, i) => <option key={d} value={i}>{d}</option>)}</Select></Field>}
        </div>
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Keep the newest" hint="0 keeps every backup."><Input type="number" min={0} value={retention.keep_last} onChange={(e) => setRetention({ ...retention, keep_last: Number(e.target.value) })} /></Field>
          <Field label="Delete backups older than (days)" hint="0 means no age limit."><Input type="number" min={0} value={retention.max_age_days} onChange={(e) => setRetention({ ...retention, max_age_days: Number(e.target.value) })} /></Field>
        </div>
        <div className="rounded-xl border border-line p-3">
          <Switch checked={encrypt} onChange={setEncrypt} label={<span><b>Encrypt backups</b> (recommended)</span>} />
          {encrypt ? (
            <div className="mt-3 grid gap-3 sm:grid-cols-2">
              <Field label="Passphrase" hint="At least 12 characters. Needed to restore."><Input type="password" value={pass} onChange={(e) => setPass(e.target.value)} onFocus={() => { if (pass === KEEP) { setPass(""); setPass2(""); } }} /></Field>
              <Field label="Confirm passphrase" hint={pass2 && pass !== pass2 ? "Passphrases don't match" : undefined}><Input type="password" value={pass2} onChange={(e) => setPass2(e.target.value)} /></Field>
            </div>
          ) : <p className="mt-2 text-xs text-warn">Unencrypted backups contain the key that decrypts every user's password. Anyone who gets the file gets your directory.</p>}
        </div>
        <Switch checked={enabled} onChange={setEnabled} label="Plan is active (scheduled backups run)" />
      </div>
    </Dialog>
  );
}

function RestoreDialog({ plan, onClose }: { plan: Plan | null; onClose: () => void }) {
  const toast = useToast();
  const files = useQuery<RemoteFile[]>(plan ? `/backups/plans/${plan.id}/files` : null);
  const [pick, setPick] = useState("");
  const [fileB64, setFileB64] = useState<string | null>(null);
  const [fileName, setFileName] = useState("");
  const [pass, setPass] = useState("");
  const [verified, setVerified] = useState<Verified | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => { setVerified(null); }, [pick, fileB64, pass]);
  const body = (verify_only: boolean) => plan ? { name: pick, passphrase: pass || null, verify_only } : { file: fileB64, passphrase: pass || null, verify_only };
  const path = plan ? `/backups/plans/${plan.id}/restore` : "/backups/restore";
  const ready = plan ? !!pick : !!fileB64;
  const check = async () => {
    setBusy(true);
    try { const r = await post<{ verified: Verified }>(path, body(true)); setVerified(r.verified); } catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
  };
  return (
    <Dialog open onClose={onClose} wide title={plan ? `Restore from ${plan.name}` : "Restore from a file"}
      footer={done ? <Button variant="primary" onClick={onClose}>Close</Button> : <><Button onClick={onClose}>Cancel</Button>
        {verified ? <Button variant="danger" onClick={() => setConfirm(true)}>Restore this backup…</Button> : <Button variant="primary" loading={busy} disabled={!ready} onClick={check}>Check backup</Button>}</>}>
      {done ? <Callout tone="good" title="Restore staged">{done}</Callout> : (
        <div className="space-y-4">
          {plan ? (files.loading ? <Loading /> : files.error ? <Callout tone="warn">{files.error}</Callout> : (files.data ?? []).length === 0 ? <p className="text-sm text-ink-3">No backups at this destination yet.</p> : (
            <div className="max-h-56 divide-y divide-line overflow-y-auto rounded-lg border border-line">
              {files.data!.map((f) => (
                <label key={f.name} className={cx("flex cursor-pointer items-center gap-3 px-3 py-2 text-sm", pick === f.name && "bg-accent-soft/50")}>
                  <input type="radio" checked={pick === f.name} onChange={() => setPick(f.name)} />
                  <span className="flex-1 truncate font-mono text-xs">{f.name}</span>
                  <span className="text-xs text-ink-3">{f.size ? `${(f.size / 1048576).toFixed(1)} MB` : ""} {f.modified ? ago(f.modified) : ""}</span>
                </label>
              ))}
            </div>
          )) : (
            <div>
              <input ref={input} type="file" accept=".odbackup" className="hidden" onChange={async (e) => {
                const f = e.target.files?.[0]; if (!f) return;
                const buf = new Uint8Array(await f.arrayBuffer());
                let s = ""; for (let i = 0; i < buf.length; i += 0x8000) s += String.fromCharCode(...buf.subarray(i, i + 0x8000));
                setFileB64(btoa(s)); setFileName(f.name);
              }} />
              <Button icon={<Upload className="size-4" />} onClick={() => input.current?.click()}>{fileName || "Choose a .odbackup file"}</Button>
            </div>
          )}
          <Field label="Passphrase" hint={plan?.has_passphrase ? "Leave blank to use the plan's passphrase." : "Only needed for encrypted backups."}><Input type="password" value={pass} onChange={(e) => setPass(e.target.value)} /></Field>
          {verified && (
            <Callout tone="good" title="This backup is intact">
              {verified.header.organization} ({verified.header.domain}), made {dateTime(verified.header.created_at)} by version {verified.header.server_version}: {verified.users} users, {verified.groups} groups, {verified.endpoints} endpoints, {verified.policies} GPOs.
            </Callout>
          )}
        </div>
      )}
      <Confirm open={confirm} onClose={() => setConfirm(false)} danger title="Replace the current directory?" confirmLabel="Stage restore"
        body="Everything changed since this backup was made is replaced when the Management Server restarts. The current data is kept in a pre-restore folder in the data directory."
        onConfirm={async () => {
          try { const r = await post<{ message: string }>(path, body(false)); setDone(`${r.message} In the desktop app: Settings → Disconnect, then host the directory again. For a standalone server, restart its service.`); }
          catch (e) { toast("bad", (e as Error).message); }
        }} />
    </Dialog>
  );
}

function DownloadDialog({ onClose }: { onClose: () => void }) {
  const toast = useToast();
  const [encrypt, setEncrypt] = useState(true);
  const [pass, setPass] = useState("");
  const [pass2, setPass2] = useState("");
  const [busy, setBusy] = useState(false);
  const ok = !encrypt || (pass.length >= 12 && pass === pass2);
  return (
    <Dialog open onClose={onClose} title="Download a backup now"
      footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" loading={busy} disabled={!ok} icon={<Download className="size-4" />} onClick={async () => {
        setBusy(true);
        try {
          const r = await post<{ file_name: string; data: string }>("/backups/download", { passphrase: encrypt ? pass : null });
          const where = await saveBinary(r.file_name, r.data);
          toast("good", where ? `Saved to ${where}` : `Downloaded ${r.file_name}`); onClose();
        } catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
      }}>Download</Button></>}>
      <div className="space-y-3">
        <Switch checked={encrypt} onChange={setEncrypt} label={<span><b>Encrypt</b> (recommended)</span>} />
        {encrypt ? (
          <div className="grid gap-3 sm:grid-cols-2">
            <Field label="Passphrase" hint="At least 12 characters"><Input type="password" value={pass} onChange={(e) => setPass(e.target.value)} /></Field>
            <Field label="Confirm" hint={pass2 && pass !== pass2 ? "Doesn't match" : undefined}><Input type="password" value={pass2} onChange={(e) => setPass2(e.target.value)} /></Field>
          </div>
        ) : <p className="text-xs text-warn">The file will contain the key that decrypts every user's password.</p>}
      </div>
    </Dialog>
  );
}
