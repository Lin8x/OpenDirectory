import { useEffect, useMemo, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { KeyRound, Lock, Plus, Search, Trash2, Unlock, UserPlus, Users as UsersIcon } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { del, patch, post, put } from "../lib/api";
import type { Group, OrgUnit, User } from "../lib/types";
import { ago, dateTime } from "../lib/format";
import { useSession } from "../lib/session";
import {
  Avatar, Badge, Button, Card, Confirm, Dialog, Drawer, EmptyState, ErrorNote, Explain, Field, Input, Loading, PageHeader, Row, Select, Switch, Table, Tabs, Td, useToast,
} from "../components/ui";

export function ouPath(ous: OrgUnit[], id: string | null): string {
  if (!id) return "—";
  const parts: string[] = [];
  let cur = ous.find((o) => o.id === id);
  let guard = 0;
  while (cur && guard++ < 32) { parts.unshift(cur.name); cur = ous.find((o) => o.id === cur!.parent_id); }
  return parts.join(" / ") || "—";
}

export function OuSelect({ ous, value, onChange, none = "No OU (domain root)" }: { ous: OrgUnit[]; value: string | null; onChange: (v: string | null) => void; none?: string }) {
  const sorted = [...ous].sort((a, b) => ouPath(ous, a.id).localeCompare(ouPath(ous, b.id)));
  return (
    <Select value={value ?? ""} onChange={(e) => onChange(e.target.value || null)}>
      <option value="">{none}</option>
      {sorted.map((o) => <option key={o.id} value={o.id}>{ouPath(ous, o.id)}</option>)}
    </Select>
  );
}

export default function Users() {
  const users = useQuery<User[]>("/users");
  const groups = useQuery<Group[]>("/groups");
  const ous = useQuery<OrgUnit[]>("/ous");
  const [params, setParams] = useSearchParams();
  const [q, setQ] = useState("");
  const [filter, setFilter] = useState<"all" | "enabled" | "disabled" | "admins">("all");
  const [creating, setCreating] = useState(params.get("new") === "1");
  const selectedId = params.get("id");

  const list = useMemo(() => {
    const n = q.trim().toLowerCase();
    return (users.data ?? []).filter((u) =>
      (!n || [u.username, u.display_name, u.email, u.department, u.title].some((x) => x.toLowerCase().includes(n))) &&
      (filter === "all" || (filter === "enabled" && u.enabled) || (filter === "disabled" && !u.enabled) || (filter === "admins" && u.role === "admin")));
  }, [users.data, q, filter]);

  const selected = users.data?.find((u) => u.id === selectedId) ?? null;
  const close = () => setParams({});

  return (
    <>
      <PageHeader title="Users" term="user"
        subtitle="People who can sign in to your managed endpoints. Domain Admins can also sign in to this Admin Center."
        actions={<Button variant="primary" icon={<UserPlus className="size-4" />} onClick={() => setCreating(true)}>New user</Button>} />
      <Card padded={false}>
        <div className="flex flex-wrap items-center gap-2 border-b border-line p-3">
          <div className="relative w-72">
            <Search className="pointer-events-none absolute left-2.5 top-2 size-4 text-ink-3" />
            <Input className="pl-8" placeholder="Search name, username, email, department" value={q} onChange={(e) => setQ(e.target.value)} />
          </div>
          <div className="flex gap-1">
            {(["all", "enabled", "disabled", "admins"] as const).map((f) => (
              <Button key={f} size="sm" variant={filter === f ? "primary" : "ghost"} onClick={() => setFilter(f)}>{f === "admins" ? "Domain Admins" : f[0]!.toUpperCase() + f.slice(1)}</Button>
            ))}
          </div>
          <span className="ml-auto text-xs text-ink-3">{list.length} of {users.data?.length ?? 0}</span>
        </div>
        {users.loading && !users.data ? <Loading /> : users.error ? <div className="p-4"><ErrorNote message={users.error} onRetry={users.reload} /></div> : (
          <Table head={["Name", "Username", "Department", "OU", "Status", "Last sign-in"]}
            empty={list.length === 0 && <EmptyState icon={<UsersIcon className="size-5" />} title={q ? "No matching users" : "No users yet"} body="Create accounts for the people in your organization." action={<Button variant="primary" icon={<Plus className="size-4" />} onClick={() => setCreating(true)}>New user</Button>} />}>
            {list.map((u) => (
              <Row key={u.id} onClick={() => setParams({ id: u.id })} selected={u.id === selectedId}>
                <Td><div className="flex items-center gap-2.5"><Avatar name={u.display_name} /><div><div className="font-medium">{u.display_name}</div><div className="text-xs text-ink-3">{u.email || u.title}</div></div></div></Td>
                <Td className="font-mono text-[13px] text-ink-2">{u.username}</Td>
                <Td className="text-ink-2">{u.department || "—"}</Td>
                <Td className="text-ink-2">{ouPath(ous.data ?? [], u.ou_id)}</Td>
                <Td>
                  <div className="flex gap-1.5">
                    {u.enabled ? <Badge tone="good">Enabled</Badge> : <Badge tone="neutral">Disabled</Badge>}
                    {u.role === "admin" && <Badge tone="accent">Domain Admin</Badge>}
                    {!u.password_set_at && <Badge tone="warn">No password</Badge>}
                  </div>
                </Td>
                <Td className="text-ink-3">{ago(u.last_login_at)}</Td>
              </Row>
            ))}
          </Table>
        )}
      </Card>

      <CreateUser open={creating} onClose={() => { setCreating(false); if (params.get("new")) setParams({}); }} groups={groups.data ?? []} ous={ous.data ?? []}
        onCreated={(u) => { void users.reload(); void groups.reload(); setCreating(false); setParams({ id: u.id }); }} />
      {selected && <UserDrawer user={selected} groups={groups.data ?? []} ous={ous.data ?? []} onClose={close} onChanged={() => { void users.reload(); void groups.reload(); }} />}
    </>
  );
}

function CreateUser({ open, onClose, groups, ous, onCreated }: { open: boolean; onClose: () => void; groups: Group[]; ous: OrgUnit[]; onCreated: (u: User) => void }) {
  const toast = useToast();
  const blank = { display_name: "", username: "", email: "", title: "", department: "", password: "", ou_id: null as string | null, admin: false, group_ids: [] as string[] };
  const [f, setF] = useState(blank);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => { if (open) { setF(blank); setErr(null); } }, [open]); // eslint-disable-line react-hooks/exhaustive-deps
  const set = (k: keyof typeof f, v: unknown) => setF((x) => ({ ...x, [k]: v }));
  const suggest = (name: string) => {
    const p = name.toLowerCase().replace(/[^a-z\s]/g, "").trim().split(/\s+/);
    return p.length > 1 ? `${p[0]}.${p[p.length - 1]}`.slice(0, 20) : (p[0] ?? "");
  };
  const submit = async () => {
    setBusy(true); setErr(null);
    try {
      const u = await post<User>("/users", { ...f, role: f.admin ? "admin" : "user", password: f.password || undefined });
      toast("good", `Created ${u.display_name}`);
      onCreated(u);
    } catch (e) { setErr((e as Error).message); } finally { setBusy(false); }
  };
  const extra = groups.filter((g) => g.name !== "Domain Users");
  return (
    <Dialog open={open} onClose={onClose} title="New user" wide
      footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" loading={busy} onClick={submit} disabled={!f.username}>Create user</Button></>}>
      <Explain term="user" />
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label="Full name"><Input autoFocus value={f.display_name} onChange={(e) => { const v = e.target.value; setF((x) => ({ ...x, display_name: v, username: !x.username || x.username === suggest(x.display_name) ? suggest(v) : x.username })); }} placeholder="Alex Morgan" /></Field>
        <Field label="Username" hint="Used to sign in on every endpoint. Max 20 characters."><Input value={f.username} onChange={(e) => set("username", e.target.value.toLowerCase())} placeholder="alex.morgan" /></Field>
        <Field label="Email"><Input value={f.email} onChange={(e) => set("email", e.target.value)} placeholder="alex@contoso.com" /></Field>
        <Field label="Initial password" hint="At least 8 characters. Leave empty to set one later."><Input type="password" value={f.password} onChange={(e) => set("password", e.target.value)} /></Field>
        <Field label="Job title"><Input value={f.title} onChange={(e) => set("title", e.target.value)} /></Field>
        <Field label="Department"><Input value={f.department} onChange={(e) => set("department", e.target.value)} /></Field>
        <Field label="Organizational unit" term="ou"><OuSelect ous={ous} value={f.ou_id} onChange={(v) => set("ou_id", v)} /></Field>
        <Field label="Role" term="domain_admin"><div className="pt-1.5"><Switch checked={f.admin} onChange={(v) => set("admin", v)} label="Domain Admin (can use the Admin Center)" /></div></Field>
      </div>
      {extra.length > 0 && (
        <Field label="Security groups" term="group" hint="Every user is also added to Domain Users.">
          <div className="flex flex-wrap gap-1.5">
            {extra.map((g) => {
              const on = f.group_ids.includes(g.id);
              return <button key={g.id} type="button" onClick={() => set("group_ids", on ? f.group_ids.filter((x) => x !== g.id) : [...f.group_ids, g.id])}
                className={`rounded-full border px-2.5 py-1 text-xs font-medium transition ${on ? "border-accent bg-accent-soft text-accent" : "border-line text-ink-2 hover:border-ink-3"}`}>{g.name}</button>;
            })}
          </div>
        </Field>
      )}
      {err && <ErrorNote message={err} />}
    </Dialog>
  );
}

function UserDrawer({ user, groups, ous, onClose, onChanged }: { user: User; groups: Group[]; ous: OrgUnit[]; onClose: () => void; onChanged: () => void }) {
  const toast = useToast();
  const { me } = useSession();
  const [tab, setTab] = useState<"general" | "account" | "groups">("general");
  const [f, setF] = useState(user);
  const [saving, setSaving] = useState(false);
  const [pwOpen, setPwOpen] = useState(false);
  const [confirmDel, setConfirmDel] = useState(false);
  useEffect(() => setF(user), [user]);
  const dirty = JSON.stringify({ ...f, group_ids: [] }) !== JSON.stringify({ ...user, group_ids: [] });

  const save = async (body: Partial<User>, msg = "Saved") => {
    setSaving(true);
    try { await patch(`/users/${user.id}`, body); toast("good", msg); onChanged(); } catch (e) { toast("bad", (e as Error).message); } finally { setSaving(false); }
  };

  return (
    <Drawer open onClose={onClose} title={<span className="flex items-center gap-3"><Avatar name={user.display_name} className="size-9" />{user.display_name}</span>}
      subtitle={<span className="font-mono">{user.username}</span>}
      actions={<>
        <Button size="sm" icon={<KeyRound className="size-3.5" />} onClick={() => setPwOpen(true)}>Reset password</Button>
        <Button size="sm" icon={user.enabled ? <Lock className="size-3.5" /> : <Unlock className="size-3.5" />} onClick={() => save({ enabled: !user.enabled }, user.enabled ? "Account disabled" : "Account enabled")}>{user.enabled ? "Disable" : "Enable"}</Button>
      </>}>
      <div className="mb-4 flex flex-wrap gap-1.5">
        {user.enabled ? <Badge tone="good">Enabled</Badge> : <Badge>Disabled</Badge>}
        {user.role === "admin" && <Badge tone="accent">Domain Admin</Badge>}
        {user.must_change_password && <Badge tone="warn">Must change password</Badge>}
      </div>
      <Tabs value={tab} onChange={setTab} tabs={[{ id: "general", label: "General" }, { id: "account", label: "Account" }, { id: "groups", label: `Member Of (${user.group_ids.length})` }]} />
      {tab === "general" && (
        <div className="space-y-4">
          <Field label="Full name"><Input value={f.display_name} onChange={(e) => setF({ ...f, display_name: e.target.value })} /></Field>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="Email"><Input value={f.email} onChange={(e) => setF({ ...f, email: e.target.value })} /></Field>
            <Field label="Job title"><Input value={f.title} onChange={(e) => setF({ ...f, title: e.target.value })} /></Field>
            <Field label="Department"><Input value={f.department} onChange={(e) => setF({ ...f, department: e.target.value })} /></Field>
            <Field label="Organizational unit" term="ou"><OuSelect ous={ous} value={f.ou_id} onChange={(v) => setF({ ...f, ou_id: v })} /></Field>
          </div>
          <div className="flex justify-end"><Button variant="primary" disabled={!dirty} loading={saving}
            onClick={() => save({ display_name: f.display_name, email: f.email, title: f.title, department: f.department, ou_id: f.ou_id })}>Save changes</Button></div>
        </div>
      )}
      {tab === "account" && (
        <div className="space-y-5">
          <dl className="grid grid-cols-2 gap-x-6 gap-y-3 text-sm">
            <Info label="Username" value={<span className="font-mono">{user.username}</span>} />
            <Info label="Sign in as" value={<span className="font-mono">{me?.domain?.split(".")[0]?.toUpperCase()}\{user.username}</span>} />
            <Info label="Password last set" value={dateTime(user.password_set_at)} />
            <Info label="Last Admin Center sign-in" value={dateTime(user.last_login_at)} />
            <Info label="Created" value={dateTime(user.created_at)} />
          </dl>
          <div className="space-y-3 rounded-lg border border-line p-4">
            <Switch checked={user.role === "admin"} onChange={(v) => save({ role: v ? "admin" : "user" }, v ? "Now a Domain Admin" : "No longer a Domain Admin")} label="Domain Admin" />
            <Switch checked={user.must_change_password} onChange={(v) => save({ must_change_password: v })} label="User must change password at next sign-in" />
          </div>
          <Explain>Disabling an account blocks Admin Center access immediately and locks the user's sign-in accounts on every endpoint at their next check-in (within about 15 seconds). Nothing is deleted.</Explain>
          <div className="border-t border-line pt-4">
            <Button variant="danger" icon={<Trash2 className="size-4" />} onClick={() => setConfirmDel(true)} disabled={user.id === me?.user.id}>Delete user</Button>
          </div>
        </div>
      )}
      {tab === "groups" && <MemberOf user={user} groups={groups} onChanged={onChanged} />}

      <ResetPassword open={pwOpen} user={user} onClose={() => setPwOpen(false)} onDone={onChanged} />
      <Confirm open={confirmDel} onClose={() => setConfirmDel(false)} danger confirmLabel="Delete user" title={`Delete ${user.display_name}?`}
        body={<>The account is removed from the directory. Local sign-in accounts already created on endpoints stay on those computers but are no longer managed. To keep history, consider disabling the account instead.</>}
        onConfirm={async () => { try { await del(`/users/${user.id}`); toast("good", "User deleted"); onChanged(); onClose(); } catch (e) { toast("bad", (e as Error).message); } }} />
    </Drawer>
  );
}

function Info({ label, value }: { label: string; value: React.ReactNode }) {
  return <div><dt className="text-xs text-ink-3">{label}</dt><dd className="mt-0.5">{value}</dd></div>;
}

function MemberOf({ user, groups, onChanged }: { user: User; groups: Group[]; onChanged: () => void }) {
  const toast = useToast();
  const [sel, setSel] = useState(user.group_ids);
  const [busy, setBusy] = useState(false);
  useEffect(() => setSel(user.group_ids), [user.group_ids]);
  const dirty = [...sel].sort().join() !== [...user.group_ids].sort().join();
  return (
    <div className="space-y-3">
      <Explain term="group" />
      <div className="divide-y divide-line rounded-lg border border-line">
        {groups.map((g) => (
          <label key={g.id} className="flex cursor-pointer items-center gap-3 px-3 py-2.5 hover:bg-panel-2">
            <input type="checkbox" className="size-4 accent-[var(--accent)]" checked={sel.includes(g.id)} onChange={(e) => setSel(e.target.checked ? [...sel, g.id] : sel.filter((x) => x !== g.id))} />
            <div className="flex-1"><div className="text-sm font-medium">{g.name}</div><div className="text-xs text-ink-3">{g.description || `${g.member_ids.length} members`}</div></div>
          </label>
        ))}
      </div>
      <div className="flex justify-end"><Button variant="primary" disabled={!dirty} loading={busy} onClick={async () => {
        setBusy(true);
        try { await put(`/users/${user.id}/groups`, { ids: sel }); toast("good", "Group membership updated"); onChanged(); } catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
      }}>Save membership</Button></div>
    </div>
  );
}

function ResetPassword({ open, user, onClose, onDone }: { open: boolean; user: User; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const [pw, setPw] = useState("");
  const [must, setMust] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => { if (open) { setPw(""); setMust(false); } }, [open]);
  const generate = () => {
    const chars = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789!#%+=";
    const a = new Uint32Array(16); crypto.getRandomValues(a);
    setPw(Array.from(a, (n) => chars[n % chars.length]).join(""));
  };
  return (
    <Dialog open={open} onClose={onClose} title={`Reset password for ${user.display_name}`}
      footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" loading={busy} disabled={pw.length < 8} onClick={async () => {
        setBusy(true);
        try { await post(`/users/${user.id}/password`, { password: pw, must_change_password: must }); toast("good", "Password reset. Endpoints update on their next check-in."); onDone(); onClose(); }
        catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
      }}>Reset password</Button></>}>
      <Field label="New password" hint="At least 8 characters.">
        <div className="flex gap-2"><Input className="font-mono" value={pw} onChange={(e) => setPw(e.target.value)} autoFocus /><Button onClick={generate}>Generate</Button></div>
      </Field>
      <Switch checked={must} onChange={setMust} label="User must change password at next sign-in" />
      <Explain>The new password is synced to every endpoint where this user has a sign-in account, within about 15 seconds for online endpoints.</Explain>
    </Dialog>
  );
}
