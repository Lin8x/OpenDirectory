import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { KeyRound, Lock, Plus, Trash2, Unlock, UserPlus, Users as UsersIcon } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { del, patch, post, put } from "../lib/api";
import type { Group, OrgUnit, User } from "../lib/types";
import { ago, dateTime } from "../lib/format";
import { useSession } from "../lib/session";
import {
  Avatar, Badge, Button, Card, Confirm, Dialog, Drawer, EmptyState, ErrorNote, Explain, Field, Input, Loading, PageHeader, Row, Select, Switch, Table, Tabs, Td, useToast, CopyButton,
} from "../components/ui";
import { ListToolbar, NoMatches, useListView, type FilterDef } from "../components/DataView";

export function ouPath(ous: OrgUnit[], id: string | null): string {
  if (!id) return "—";
  const parts: string[] = [];
  let cur = ous.find((o) => o.id === id);
  let guard = 0;
  while (cur && guard++ < 32) { parts.unshift(cur.name); cur = ous.find((o) => o.id === cur!.parent_id); }
  return parts.join(" / ") || "—";
}

/** OU filter for useListView: every OU path, plus objects at the domain root. */
export function ouFilter<T>(ous: OrgUnit[], get: (r: T) => string | null): FilterDef<T> {
  return { kind: "enum", key: "ou", label: "OU", get: (r) => get(r) ?? "__root", format: (id) => (id === "__root" ? "No OU (domain root)" : ouPath(ous, id)) };
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
  const [creating, setCreating] = useState(params.get("new") === "1");
  const selectedId = params.get("id");
  const ouList = ous.data ?? [];
  const view = useListView(users.data, {
    id: "users",
    search: (u) => [u.username, u.display_name, u.email, u.department, u.title],
    filters: [
      { kind: "enum", key: "status", label: "Status", get: (u) => (u.enabled ? "enabled" : "disabled"), options: [{ value: "enabled", label: "Enabled" }, { value: "disabled", label: "Disabled" }] },
      { kind: "enum", key: "role", label: "Role", get: (u) => (u.role === "admin" ? "admin" : "user"), options: [{ value: "admin", label: "Domain Admin" }, { value: "user", label: "Standard user" }] },
      { kind: "enum", key: "password", label: "Password", get: (u) => (u.password_set_at ? "set" : "none"), options: [{ value: "set", label: "Password set" }, { value: "none", label: "No password" }] },
      { kind: "enum", key: "department", label: "Department", get: (u) => u.department || null },
      ouFilter(ouList, (u: User) => u.ou_id),
      { kind: "date", key: "login", label: "Last sign-in", get: (u) => u.last_login_at },
    ],
    sorts: {
      name: { label: "Name", get: (u) => u.display_name },
      username: { label: "Username", get: (u) => u.username },
      department: { label: "Department", get: (u) => u.department },
      ou: { label: "OU", get: (u) => (u.ou_id ? ouPath(ouList, u.ou_id) : null) },
      status: { label: "Status", get: (u) => (u.enabled ? 0 : 2) + (u.role === "admin" ? 0 : 1), type: "number" },
      login: { label: "Last sign-in", get: (u) => u.last_login_at, type: "date" },
    },
    defaultSort: { key: "name", dir: "asc" },
  });
  const list = view.rows;

  const selected = users.data?.find((u) => u.id === selectedId) ?? null;
  const close = () => setParams({});

  return (
    <>
      <PageHeader title="Users" term="user"
        subtitle="People who can sign in to your managed endpoints. Domain Admins can also sign in to this Admin Center."
        actions={<Button variant="primary" icon={<UserPlus className="size-4" />} onClick={() => setCreating(true)}>New user</Button>} />
      <Card padded={false}>
        <ListToolbar view={view} placeholder="Search name, username, email, department" noun={["user", "users"]} />
        {users.loading && !users.data ? <Loading /> : users.error ? <div className="p-4"><ErrorNote message={users.error} onRetry={users.reload} /></div> : (
          <Table view={view} head={[{ label: "Name", sort: "name" }, { label: "Username", sort: "username" }, { label: "Department", sort: "department" }, { label: "OU", sort: "ou" }, { label: "Status", sort: "status" }, { label: "Last sign-in", sort: "login" }]}
            empty={list.length === 0 && (view.total > 0 ? <NoMatches view={view} /> : <EmptyState icon={<UsersIcon className="size-5" />} title="No users yet" body="Create accounts for the people in your organization." action={<Button variant="primary" icon={<Plus className="size-4" />} onClick={() => setCreating(true)}>New user</Button>} />)}>
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
  // New people land in the Staff OU when it exists, like AD's default Users container.
  useEffect(() => { if (open) { setF({ ...blank, ou_id: ous.find((o) => o.name === "Staff" && !o.parent_id)?.id ?? null }); setErr(null); } }, [open]); // eslint-disable-line react-hooks/exhaustive-deps
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
  // Domain Admins is the Role switch, and everyone is in Domain Users.
  const extra = groups.filter((g) => g.name !== "Domain Users" && g.name !== "Domain Admins");
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
        <Field label="Role" term="domain_admin" hint="Adds them to the Domain Admins group."><div className="pt-1.5"><Switch checked={f.admin} onChange={(v) => set("admin", v)} label="Domain Admin (can use the Admin Center)" /></div></Field>
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
        {groups.map((g) => {
          const builtin = g.name === "Domain Users";
          return (
            <label key={g.id} className={`flex items-center gap-3 px-3 py-2.5 ${builtin ? "opacity-70" : "cursor-pointer hover:bg-panel-2"}`}>
              <input type="checkbox" disabled={builtin} className="size-4 accent-[var(--accent)]" checked={sel.includes(g.id)} onChange={(e) => setSel(e.target.checked ? [...sel, g.id] : sel.filter((x) => x !== g.id))} />
              <div className="flex-1">
                <div className="text-sm font-medium">{g.name}</div>
                <div className="text-xs text-ink-3">
                  {builtin ? "Every user is a member" : g.name === "Domain Admins" ? "Members are Domain Admins and can use the Admin Center" : g.description || `${g.member_ids.length} members`}
                </div>
              </div>
            </label>
          );
        })}
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
      <ResultantPso userId={user.id} />
      <ResetCode user={user} />
      <Explain>The new password is synced to every endpoint where this user has a sign-in account, within about 15 seconds for online endpoints.</Explain>
    </Dialog>
  );
}

function ResultantPso({ userId }: { userId: string }) {
  const r = useQuery<{ source: string; rules: { min_length: number; complexity: boolean; history: number; max_age_days: number } }>(`/users/${userId}/resultant-password-policy`);
  if (!r.data) return null;
  const x = r.data.rules;
  return <p className="text-xs text-ink-3">Rules from <b>{r.data.source}</b>: at least {x.min_length} characters{x.complexity ? ", mixed character types" : ""}{x.history ? `, can't reuse the last ${x.history}` : ""}{x.max_age_days ? `, expires after ${x.max_age_days} days` : ""}.</p>;
}

function ResetCode({ user }: { user: User }) {
  const toast = useToast();
  const [code, setCode] = useState<{ code: string; expires_at: string } | null>(null);
  return (
    <div className="rounded-lg border border-line p-3">
      <div className="flex items-center justify-between gap-3">
        <div className="text-sm"><b>Or give them a reset code</b><p className="text-xs text-ink-3">They choose their own password with "Change password" on any sign-in screen. Works once, for 24 hours.</p></div>
        <Button size="sm" onClick={async () => { try { setCode(await post(`/users/${user.id}/reset-code`)); } catch (e) { toast("bad", (e as Error).message); } }}>Create code</Button>
      </div>
      {code && <div className="mt-2 flex items-center gap-2"><code className="selectable rounded bg-panel-2 px-2 py-1 font-mono text-base tracking-widest">{code.code}</code><CopyButton text={code.code} /></div>}
    </div>
  );
}
