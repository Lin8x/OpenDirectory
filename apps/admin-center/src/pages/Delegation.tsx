// Delegation of Control: give a user or group scoped permissions over some
// endpoints, OUs or groups, optionally until a set time (temporary admin).
import { useState } from "react";
import { Clock, Plus, ShieldHalf, Trash2, X } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { del, post, put } from "../lib/api";
import type { Endpoint, Group, OrgUnit, User } from "../lib/types";
import { ago } from "../lib/format";
import type { Permission } from "../lib/session";
import { Badge, Button, Callout, Card, Confirm, Dialog, EmptyState, ErrorNote, Field, Input, Loading, PageHeader, Select, Switch, Textarea, useToast } from "../components/ui";
import { ouPath } from "./Users";
import { PERMISSION_LABELS } from "./Operator";

interface ScopeItem { kind: "endpoint" | "ou" | "group"; id: string; include_children?: boolean }
interface Delegation {
  id: string; name: string; description: string; trustee_kind: "user" | "group"; trustee_id: string;
  scope: ScopeItem[]; permissions: Permission[]; expires_at: string | null; enabled: boolean; created_by: string; created_at: string;
}

const GROUPS: { label: string; perms: Permission[] }[] = [
  { label: "Endpoints", perms: ["view_endpoints", "remote_desktop", "remote_shell", "endpoint_actions", "run_scripts", "read_laps", "help_desk"] },
  { label: "Endpoint management", perms: ["manage_services", "manage_software", "manage_processes", "manage_containers"] },
  { label: "Accounts", perms: ["view_users", "reset_passwords"] },
];

/** Common tasks, like the AD Delegation of Control Wizard. */
const TEMPLATES: { label: string; perms: Permission[] }[] = [
  { label: "Remote desktop user", perms: ["view_endpoints", "remote_desktop"] },
  { label: "Help desk operator", perms: ["view_endpoints", "remote_desktop", "help_desk", "view_users", "reset_passwords"] },
  { label: "Endpoint operator", perms: ["view_endpoints", "remote_desktop", "remote_shell", "endpoint_actions", "run_scripts", "read_laps", "manage_services", "manage_software", "manage_processes"] },
  { label: "Read-only", perms: ["view_endpoints", "view_users"] },
];

const EXPIRY = [
  { label: "Never", hours: 0 },
  { label: "1 hour", hours: 1 },
  { label: "8 hours", hours: 8 },
  { label: "1 day", hours: 24 },
  { label: "1 week", hours: 24 * 7 },
  { label: "30 days", hours: 24 * 30 },
];

export default function DelegationPage() {
  const list = useQuery<Delegation[]>("/delegations");
  const users = useQuery<User[]>("/users");
  const groups = useQuery<Group[]>("/groups");
  const ous = useQuery<OrgUnit[]>("/ous");
  const eps = useQuery<Endpoint[]>("/endpoints");
  const [edit, setEdit] = useState<Delegation | "new" | null>(null);
  const [remove, setRemove] = useState<Delegation | null>(null);
  const toast = useToast();
  if (list.loading && !list.data) return <Loading />;
  if (list.error && !list.data) return <ErrorNote message={list.error} onRetry={list.reload} />;
  const trustee = (d: Delegation) => d.trustee_kind === "user"
    ? (users.data?.find((u) => u.id === d.trustee_id)?.username ?? "deleted user")
    : (groups.data?.find((g) => g.id === d.trustee_id)?.name ?? "deleted group");
  const scopeName = (i: ScopeItem) => i.kind === "endpoint" ? (eps.data?.find((e) => e.id === i.id)?.hostname ?? "deleted endpoint")
    : i.kind === "ou" ? `${ouPath(ous.data ?? [], i.id)}${i.include_children === false ? "" : " (and child OUs)"}`
    : `Group: ${groups.data?.find((g) => g.id === i.id)?.name ?? "deleted"}`;
  const now = Date.now();
  return (
    <div className="space-y-5">
      <PageHeader title="Delegation of Control" subtitle="Let people who aren't Domain Admins manage only the devices and accounts you choose. They see nothing else."
        actions={<Button variant="primary" icon={<Plus className="size-4" />} onClick={() => setEdit("new")}>Delegate control</Button>} />
      <Callout>Delegated operators sign in to this console (in the desktop app or the web console) with their own account. Endpoints, users and groups outside their scope are hidden from them everywhere. Require two-factor sign-in for them in <b>Settings</b>.</Callout>
      {(list.data ?? []).length === 0 ? (
        <EmptyState icon={<ShieldHalf className="size-6" />} title="Nothing delegated yet" body="For example, give a family member remote desktop to the gaming PC, or a technician help desk rights over one office's OU for a week." action={<Button variant="primary" onClick={() => setEdit("new")}>Delegate control</Button>} />
      ) : (
        <div className="grid gap-3">
          {list.data!.map((d) => {
            const expired = d.expires_at && new Date(d.expires_at).getTime() <= now;
            return (
              <Card key={d.id} title={<span className="flex items-center gap-2">{d.name}{!d.enabled && <Badge>Off</Badge>}{expired && <Badge tone="warn">Expired</Badge>}</span>}
                subtitle={<>To <b>{d.trustee_kind === "group" ? "group " : ""}{trustee(d)}</b>{d.expires_at && !expired ? <> · <Clock className="inline size-3" /> until {new Date(d.expires_at).toLocaleString()}</> : null}</>}
                actions={<div className="flex gap-1"><Button size="sm" onClick={() => setEdit(d)}>Edit</Button><Button size="sm" variant="ghost" icon={<Trash2 className="size-3.5" />} onClick={() => setRemove(d)} /></div>}>
                <div className="space-y-2 text-sm">
                  <div className="flex flex-wrap gap-1.5">{d.permissions.map((p) => <Badge key={p} tone="accent">{PERMISSION_LABELS[p] ?? p}</Badge>)}</div>
                  <div className="text-ink-2">Over: {d.scope.map(scopeName).join(", ")}</div>
                  {d.description && <div className="text-ink-3">{d.description}</div>}
                  <div className="text-xs text-ink-3">Created by {d.created_by} {ago(d.created_at)}</div>
                </div>
              </Card>
            );
          })}
        </div>
      )}
      {edit && <Editor initial={edit === "new" ? null : edit} users={users.data ?? []} groups={groups.data ?? []} ous={ous.data ?? []} eps={eps.data ?? []}
        onClose={() => setEdit(null)} onSaved={() => { setEdit(null); list.reload(); }} />}
      <Confirm open={!!remove} onClose={() => setRemove(null)} danger title={`Remove "${remove?.name}"?`} confirmLabel="Remove"
        body="Their access ends immediately; open sessions are signed out on their next request."
        onConfirm={async () => { await del(`/delegations/${remove!.id}`); toast("good", "Delegation removed"); list.reload(); }} />
    </div>
  );
}

function Editor({ initial, users, groups, ous, eps, onClose, onSaved }: {
  initial: Delegation | null; users: User[]; groups: Group[]; ous: OrgUnit[]; eps: Endpoint[]; onClose: () => void; onSaved: () => void;
}) {
  const toast = useToast();
  const [name, setName] = useState(initial?.name ?? "");
  const [description, setDescription] = useState(initial?.description ?? "");
  const [tKind, setTKind] = useState<"user" | "group">(initial?.trustee_kind ?? "user");
  const [tId, setTId] = useState(initial?.trustee_id ?? "");
  const [scope, setScope] = useState<ScopeItem[]>(initial?.scope ?? []);
  const [perms, setPerms] = useState<Permission[]>(initial?.permissions ?? ["view_endpoints", "remote_desktop"]);
  const [expires, setExpires] = useState<string | null>(initial?.expires_at ?? null);
  const [enabled, setEnabled] = useState(initial?.enabled ?? true);
  const [addKind, setAddKind] = useState<ScopeItem["kind"]>("endpoint");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const trustees = tKind === "user" ? users.filter((u) => u.role !== "admin") : groups.filter((g) => g.name !== "Domain Admins");
  const options = addKind === "endpoint" ? eps.map((e) => ({ id: e.id, label: e.hostname }))
    : addKind === "ou" ? ous.map((o) => ({ id: o.id, label: ouPath(ous, o.id) })).sort((a, b) => a.label.localeCompare(b.label))
    : groups.map((g) => ({ id: g.id, label: g.name }));
  const label = (i: ScopeItem) => i.kind === "endpoint" ? `Endpoint: ${eps.find((e) => e.id === i.id)?.hostname ?? "?"}` : i.kind === "ou" ? `OU: ${ouPath(ous, i.id)}` : `Group: ${groups.find((g) => g.id === i.id)?.name ?? "?"}`;
  const toggle = (p: Permission) => setPerms((ps) => ps.includes(p) ? ps.filter((x) => x !== p) : [...ps, p]);
  const save = async () => {
    setBusy(true); setErr(null);
    const body = { name, description, trustee_kind: tKind, trustee_id: tId, scope, permissions: perms, expires_at: expires, enabled };
    try {
      if (initial) await put(`/delegations/${initial.id}`, body); else await post("/delegations", body);
      toast("good", initial ? "Delegation saved" : "Control delegated");
      onSaved();
    } catch (e) { setErr((e as Error).message); } finally { setBusy(false); }
  };
  return (
    <Dialog open onClose={onClose} wide title={initial ? `Edit ${initial.name}` : "Delegate control"}
      footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" loading={busy} disabled={!name.trim() || !tId || !scope.length || !perms.length} onClick={save}>{initial ? "Save" : "Delegate"}</Button></>}>
      <div className="space-y-5">
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Name"><Input value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Gaming PC for Alex" autoFocus /></Field>
          <Field label="Delegate to">
            <div className="flex gap-2">
              <Select value={tKind} onChange={(e) => { setTKind(e.target.value as "user" | "group"); setTId(""); }}><option value="user">User</option><option value="group">Group</option></Select>
              <Select value={tId} onChange={(e) => setTId(e.target.value)}>
                <option value="">Choose…</option>
                {trustees.map((t) => <option key={t.id} value={t.id}>{"username" in t ? `${t.username}${t.display_name ? ` (${t.display_name})` : ""}` : t.name}</option>)}
              </Select>
            </div>
          </Field>
        </div>

        <div>
          <div className="mb-1.5 text-sm font-medium">Over these objects</div>
          <div className="flex flex-wrap gap-1.5">
            {scope.map((i) => (
              <span key={`${i.kind}${i.id}`} className="inline-flex items-center gap-1.5 rounded-full border border-line bg-panel-2 px-2.5 py-1 text-xs">
                {label(i)}
                {i.kind === "ou" && <label className="flex items-center gap-1 text-ink-3"><input type="checkbox" checked={i.include_children !== false} onChange={(e) => setScope((s) => s.map((x) => x === i ? { ...x, include_children: e.target.checked } : x))} />child OUs</label>}
                <button onClick={() => setScope((s) => s.filter((x) => x !== i))}><X className="size-3" /></button>
              </span>
            ))}
            {!scope.length && <span className="text-xs text-ink-3">Nothing yet.</span>}
          </div>
          <div className="mt-2 flex gap-2">
            <Select value={addKind} onChange={(e) => setAddKind(e.target.value as ScopeItem["kind"])}><option value="endpoint">Endpoint</option><option value="ou">OU</option><option value="group">Security group</option></Select>
            <Select value="" onChange={(e) => { const id = e.target.value; if (id && !scope.some((s) => s.kind === addKind && s.id === id)) setScope((s) => [...s, { kind: addKind, id, ...(addKind === "ou" ? { include_children: true } : {}) }]); }}>
              <option value="">Add…</option>
              {options.map((o) => <option key={o.id} value={o.id}>{o.label}</option>)}
            </Select>
          </div>
          <p className="mt-1 text-xs text-ink-3">An OU covers its endpoints and users; a group covers its member computers and users. Domain Admins are never in a delegated scope.</p>
        </div>

        <div>
          <div className="mb-1.5 flex flex-wrap items-center gap-2 text-sm font-medium">Permissions
            <span className="text-xs font-normal text-ink-3">Start from:</span>
            {TEMPLATES.map((t) => <button key={t.label} type="button" className="rounded-full border border-line px-2 py-0.5 text-xs font-normal hover:bg-panel-2" onClick={() => setPerms(t.perms)}>{t.label}</button>)}
          </div>
          <div className="grid gap-3 sm:grid-cols-3">
            {GROUPS.map((g) => (
              <div key={g.label} className="rounded-lg border border-line p-3">
                <div className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-ink-3">{g.label}</div>
                {g.perms.map((p) => (
                  <label key={p} className="flex items-center gap-2 py-0.5 text-sm"><input type="checkbox" checked={perms.includes(p)} onChange={() => toggle(p)} />{PERMISSION_LABELS[p]}</label>
                ))}
              </div>
            ))}
          </div>
        </div>

        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Access ends" hint={expires ? `On ${new Date(expires).toLocaleString()}` : "Until you remove it"}>
            <Select value="" onChange={(e) => { const h = Number(e.target.value); setExpires(h ? new Date(Date.now() + h * 3600_000).toISOString() : null); }}>
              <option value="">{expires ? "Keep current expiry" : "Choose…"}</option>
              {EXPIRY.map((x) => <option key={x.label} value={x.hours}>{x.label === "Never" ? "Never" : `In ${x.label}`}</option>)}
            </Select>
          </Field>
          <Field label="Notes (optional)"><Textarea rows={1} value={description} onChange={(e) => setDescription(e.target.value)} /></Field>
        </div>
        <Switch checked={enabled} onChange={setEnabled} label="Enabled" />
        {err && <Callout tone="warn">{err}</Callout>}
      </div>
    </Dialog>
  );
}
