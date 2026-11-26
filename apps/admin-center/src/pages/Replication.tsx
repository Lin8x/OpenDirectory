// Replication & Failover: replica servers that copy the primary and keep
// serving agents, sign-in, DNS, RADIUS and LDAP if the primary goes down.
import { AlertTriangle, ArrowUpCircle, CopyPlus, RefreshCw, Server, Trash2 } from "lucide-react";
import { useState } from "react";
import { del, post } from "../lib/api";
import { ago } from "../lib/format";
import { useQuery } from "../lib/hooks";
import { Badge, Button, Callout, Card, CodeBlock, Confirm, Dialog, EmptyState, Explain, Field, Input, Loading, PageHeader, Textarea, useToast } from "../components/ui";

interface Peer { url: string; fingerprint: string | null }
export interface ReplicationStatus {
  role: "primary" | "replica";
  this_server: Peer;
  epoch: number;
  primary: Peer | null;
  replicas: { id: string; name: string; url: string | null; created_at: string; last_seen: string | null; online: boolean; version: string }[];
  last_sync: string | null;
  last_error: string;
  superseded: boolean;
  sync_every_secs: number;
}

export default function Replication() {
  const st = useQuery<ReplicationStatus>("/replication", { poll: 15000 });
  const toast = useToast();
  const [adding, setAdding] = useState(false);
  const [joining, setJoining] = useState(false);
  const [promote, setPromote] = useState(false);
  const [removing, setRemoving] = useState<{ id: string; name: string } | null>(null);
  const [code, setCode] = useState<{ name: string; code: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const s = st.data;
  if (!s) return <><PageHeader title="Replication & Failover" /><Loading /></>;
  const act = async (fn: () => Promise<unknown>, ok: string) => {
    setBusy(true);
    try { await fn(); toast("good", ok); void st.reload(); } catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
  };

  return (
    <>
      <PageHeader title="Replication & Failover" subtitle="Run replica servers that copy this domain and keep endpoints, sign-in, DNS, RADIUS and LDAP working if the primary goes down."
        actions={s.role === "primary" ? <Button variant="primary" icon={<CopyPlus className="size-4" />} onClick={() => setAdding(true)}>Add replica server</Button> : undefined} />

      {s.superseded && (
        <div className="mb-5"><Callout tone="warn" title="Another server was promoted to primary">Agents report a newer primary, so changes made here no longer reach them. Make this server a replica of the new primary: create a join code there, then use <b>Make this server a replica</b> below.</Callout></div>
      )}

      <div className="mb-5 grid gap-4 md:grid-cols-3">
        <Card>
          <div className="flex items-center gap-3"><Server className="size-5 text-ink-3" /><div><div className="text-xs text-ink-3">This server</div><div className="font-medium">{s.this_server.url}</div></div></div>
        </Card>
        <Card>
          <div className="text-xs text-ink-3">Role</div>
          <div className="mt-1"><Badge tone={s.role === "primary" ? "accent" : "neutral"}>{s.role === "primary" ? "Primary (read-write)" : "Replica (read-only)"}</Badge></div>
        </Card>
        <Card>
          <div className="text-xs text-ink-3">{s.role === "primary" ? "Replica servers" : "Last copy from the primary"}</div>
          <div className="mt-1 text-sm">{s.role === "primary" ? `${s.replicas.filter((r) => r.online).length} of ${s.replicas.length} online` : s.last_sync ? ago(s.last_sync) : "Never"}</div>
        </Card>
      </div>

      {s.role === "replica" ? (
        <Card title="Primary server" actions={<div className="flex gap-2">
          <Button size="sm" loading={busy} icon={<RefreshCw className="size-3.5" />} onClick={() => act(() => post("/replication/sync"), "Copied the latest changes from the primary")}>Sync now</Button>
          <Button size="sm" variant="danger" icon={<ArrowUpCircle className="size-3.5" />} onClick={() => setPromote(true)}>Promote to primary</Button>
        </div>}>
          <div className="space-y-2 text-sm">
            <div><span className="text-ink-3">Primary:</span> {s.primary?.url}</div>
            <div><span className="text-ink-3">Copies every:</span> {Math.round(s.sync_every_secs / 60)} minute(s)</div>
            {s.last_error && <Callout tone="warn" title="The last copy failed">{s.last_error}. This replica keeps serving its last good copy.</Callout>}
          </div>
        </Card>
      ) : (
        <Card title="Replica servers" padded={false}>
          {s.replicas.length === 0 ? <EmptyState icon={<CopyPlus className="size-5" />} title="No replica servers yet" body="Add one to keep the domain running when this server is down for maintenance or fails." action={<Button onClick={() => setAdding(true)}>Add replica server</Button>} /> : (
            <ul className="divide-y divide-line">
              {s.replicas.map((r) => (
                <li key={r.id} className="flex flex-wrap items-center gap-3 px-4 py-3">
                  <span className={`size-2.5 rounded-full ${r.online ? "bg-good" : "bg-ink-3"}`} />
                  <div className="min-w-0 flex-1">
                    <div className="font-medium">{r.name} {r.version && <span className="text-xs font-normal text-ink-3">v{r.version}</span>}</div>
                    <div className="text-xs text-ink-3">{r.url ?? "Waiting for it to join"} · {r.last_seen ? `last copy ${ago(r.last_seen)}` : `added ${ago(r.created_at)}`}</div>
                  </div>
                  <Badge tone={r.online ? "good" : r.last_seen ? "warn" : "neutral"}>{r.online ? "In sync" : r.last_seen ? "Not syncing" : "Not joined"}</Badge>
                  <Button size="sm" variant="ghost" icon={<Trash2 className="size-3.5" />} aria-label={`Remove ${r.name}`} onClick={() => setRemoving({ id: r.id, name: r.name })} />
                </li>
              ))}
            </ul>
          )}
        </Card>
      )}

      <div className="mt-5 grid gap-5 lg:grid-cols-2">
        <Card title="How failover works">
          <ul className="list-disc space-y-1.5 pl-4 text-sm text-ink-2">
            <li>Each replica copies the whole directory from the primary every minute: users, passwords, groups, GPOs, scripts, DNS, RADIUS, LDAP and certificate settings.</li>
            <li>Replicas serve all the time, not only during outages. Agents learn every server and switch automatically when one can't be reached, then return to the primary when it's back.</li>
            <li>Replicas are read-only. If the primary is gone for good, promote a replica. Agents follow the new primary, and the old one is flagged so it doesn't hand out stale policy.</li>
            <li>Not on replicas: DHCP (two servers would hand out the same addresses), jobs and remote shells, and scheduled backups.</li>
          </ul>
        </Card>
        <Card title="Make this server a replica">
          <p className="text-sm text-ink-2">Use a join code from the primary. This replaces this server's directory with the primary's copy.</p>
          <div className="mt-3"><Button onClick={() => setJoining(true)}>Enter join code…</Button></div>
          <div className="mt-4"><Explain>On a brand-new server, choose Join an existing domain as a replica on the first-run screen instead.</Explain></div>
        </Card>
      </div>

      <AddDialog open={adding} onClose={() => setAdding(false)} onCreated={(name, c) => { setAdding(false); setCode({ name, code: c }); void st.reload(); }} />
      <Dialog open={!!code} onClose={() => setCode(null)} title={`Join code for ${code?.name ?? ""}`} wide footer={<Button variant="primary" onClick={() => setCode(null)}>Done</Button>}>
        <p className="mb-3 text-sm text-ink-2">Install OpenDirectory on the new server, open its Admin Center, choose <b>Join an existing domain as a replica</b> and paste this code. It's shown once and works only for this replica.</p>
        <CodeBlock code={code?.code ?? ""} label="Join code" />
        <div className="mt-3"><Callout tone="info">The replica must be able to reach this server at {s.this_server.url}.</Callout></div>
      </Dialog>
      <JoinDialog open={joining} replace onClose={() => setJoining(false)} onJoined={() => { setJoining(false); window.location.reload(); }} />
      <Confirm open={promote} onClose={() => setPromote(false)} danger title="Promote this replica to primary?" confirmLabel="Promote"
        body="Do this only if the primary is gone or being replaced. This server starts accepting changes, agents switch to it, and other replicas need new join codes from it."
        onConfirm={() => act(() => post("/replication/promote"), "This server is now the primary")} />
      <Confirm open={!!removing} onClose={() => setRemoving(null)} danger title={`Remove ${removing?.name}?`} confirmLabel="Remove"
        body="It stops receiving copies and agents stop using it. It keeps its last copy until you reset it."
        onConfirm={async () => { if (removing) await act(() => del(`/replication/replicas/${removing.id}`), "Replica removed"); }} />
    </>
  );
}

function AddDialog({ open, onClose, onCreated }: { open: boolean; onClose: () => void; onCreated: (name: string, code: string) => void }) {
  const [name, setName] = useState("DC02");
  const [busy, setBusy] = useState(false);
  const toast = useToast();
  return (
    <Dialog open={open} onClose={onClose} title="Add a replica server" footer={<>
      <Button variant="ghost" onClick={onClose}>Cancel</Button>
      <Button variant="primary" loading={busy} disabled={!name.trim()} onClick={async () => {
        setBusy(true);
        try { const r = await post<{ join_code: string }>("/replication/replicas", { name }); onCreated(name, r.join_code); } catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
      }}>Create join code</Button>
    </>}>
      <Field label="Server name" hint="For your reference, for example DC02 or Branch-Office."><Input value={name} onChange={(e) => setName(e.target.value)} /></Field>
    </Dialog>
  );
}

export function JoinDialog({ open, onClose, onJoined, replace }: { open: boolean; onClose: () => void; onJoined: () => void; replace?: boolean }) {
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  return (
    <Dialog open={open} onClose={onClose} title="Join an existing domain as a replica" footer={<>
      <Button variant="ghost" onClick={onClose}>Cancel</Button>
      <Button variant="primary" loading={busy} disabled={!code.trim().startsWith("ODREPL-")} onClick={async () => {
        setBusy(true); setErr(null);
        try { await post("/replication/join", { join_code: code.trim(), replace_existing: !!replace }); onJoined(); } catch (e) { setErr((e as Error).message); } finally { setBusy(false); }
      }}>Join</Button>
    </>}>
      <p className="mb-3 text-sm text-ink-2">On the primary, open Replication &amp; Failover and choose <b>Add replica server</b> to get a code.</p>
      {replace && <div className="mb-3"><Callout tone="warn">This server's current directory will be replaced by the primary's copy.</Callout></div>}
      <Field label="Join code"><Textarea rows={3} value={code} onChange={(e) => setCode(e.target.value)} placeholder="ODREPL-…" /></Field>
      {err && <p className="mt-2 flex items-center gap-1.5 text-sm text-bad"><AlertTriangle className="size-4" />{err}</p>}
    </Dialog>
  );
}
