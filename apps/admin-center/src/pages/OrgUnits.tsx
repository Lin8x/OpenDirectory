import { useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { ChevronDown, ChevronRight, Folder, FolderOpen, FolderPlus, Globe, Monitor, Pencil, ShieldCheck, Trash2, Users, UsersRound } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { del, patch, post } from "../lib/api";
import { useSession } from "../lib/session";
import type { Endpoint, Group, OrgUnit, Policy, User } from "../lib/types";
import { Avatar, Badge, Button, Card, Confirm, Dialog, Explain, Field, Input, Loading, OsIcon, PageHeader, Textarea, cx, useToast } from "../components/ui";
import { OuSelect } from "./Users";

export default function OrgUnits() {
  const { me } = useSession();
  const ous = useQuery<OrgUnit[]>("/ous");
  const users = useQuery<User[]>("/users");
  const groups = useQuery<Group[]>("/groups");
  const endpoints = useQuery<Endpoint[]>("/endpoints");
  const policies = useQuery<Policy[]>("/policies");
  const [params, setParams] = useSearchParams();
  const sel = params.get("id");
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const [editing, setEditing] = useState<OrgUnit | "new" | null>(null);
  const [confirm, setConfirm] = useState(false);
  const toast = useToast();

  const all = ous.data ?? [];
  const children = (p: string | null) => all.filter((o) => o.parent_id === p).sort((a, b) => a.name.localeCompare(b.name));
  const current = all.find((o) => o.id === sel) ?? null;

  const inOu = useMemo(() => ({
    users: (users.data ?? []).filter((u) => u.ou_id === sel),
    groups: (groups.data ?? []).filter((g) => g.ou_id === sel),
    endpoints: (endpoints.data ?? []).filter((e) => e.ou_id === sel),
    linked: (policies.data ?? []).filter((p) => (sel ? p.targets.ou_ids.includes(sel) : p.targets.all_endpoints)),
  }), [users.data, groups.data, endpoints.data, policies.data, sel]);

  const node = (o: OrgUnit, depth: number): React.ReactNode => {
    const kids = children(o.id);
    const isOpen = open[o.id] ?? true;
    return (
      <div key={o.id}>
        <div className={cx("flex h-8 items-center gap-1 rounded-md pr-2 text-[13.5px]", sel === o.id ? "bg-accent-soft font-medium text-accent" : "text-ink-2 hover:bg-panel-2")} style={{ paddingLeft: 6 + depth * 16 }}>
          <button className="grid size-5 place-items-center text-ink-3" onClick={() => setOpen({ ...open, [o.id]: !isOpen })}>{kids.length ? (isOpen ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />) : null}</button>
          <button className="flex flex-1 items-center gap-2 truncate text-left" onClick={() => setParams({ id: o.id })}>
            {sel === o.id ? <FolderOpen className="size-4 shrink-0" /> : <Folder className="size-4 shrink-0" />}<span className="truncate">{o.name}</span>
          </button>
        </div>
        {isOpen && kids.map((k) => node(k, depth + 1))}
      </div>
    );
  };

  if (ous.loading && !ous.data) return <Loading />;
  return (
    <>
      <PageHeader title="Organizational Units" term="ou" subtitle="Folders that organize users, groups and endpoints. Link GPOs to an OU and every endpoint inside it, including child OUs, receives them."
        actions={<Button variant="primary" icon={<FolderPlus className="size-4" />} onClick={() => setEditing("new")}>New OU</Button>} />
      <div className="grid gap-5 lg:grid-cols-[280px_1fr]">
        <Card padded={false} className="h-fit">
          <div className="p-2">
            <button onClick={() => setParams({})} className={cx("flex h-8 w-full items-center gap-2 rounded-md px-2 text-[13.5px]", !sel ? "bg-accent-soft font-medium text-accent" : "text-ink-2 hover:bg-panel-2")}>
              <Globe className="size-4" />{me?.domain}
            </button>
            {children(null).map((o) => node(o, 1))}
          </div>
        </Card>
        <div className="space-y-5">
          <Card title={<span className="flex items-center gap-2">{current ? <FolderOpen className="size-4 text-accent" /> : <Globe className="size-4 text-accent" />}{current?.name ?? `${me?.domain} (domain root)`}</span>}
            subtitle={current?.description || (current ? undefined : "Objects not placed in any OU")}
            actions={current && <>
              <Button size="sm" icon={<Pencil className="size-3.5" />} onClick={() => setEditing(current)}>Edit</Button>
              <Button size="sm" variant="ghost" icon={<Trash2 className="size-3.5" />} onClick={() => setConfirm(true)}>Delete</Button>
            </>}>
            <div className="grid gap-4 sm:grid-cols-3">
              <Count icon={<Users className="size-4" />} label="Users" n={inOu.users.length} />
              <Count icon={<UsersRound className="size-4" />} label="Groups" n={inOu.groups.length} />
              <Count icon={<Monitor className="size-4" />} label="Endpoints" n={inOu.endpoints.length} />
            </div>
          </Card>
          <Card title="Linked Group Policy" subtitle={current ? "GPOs linked here or to a parent OU apply to endpoints in this OU" : "GPOs that apply to all endpoints"}>
            {inOu.linked.length === 0 ? <p className="text-sm text-ink-3">No GPOs linked directly. <Link to="/gpos/new" className="text-accent hover:underline">Create one</Link>.</p> : (
              <div className="flex flex-wrap gap-2">{inOu.linked.map((p) => <Link key={p.id} to={`/gpos/${p.id}`}><Badge tone="accent"><ShieldCheck className="size-3" />{p.name}</Badge></Link>)}</div>
            )}
          </Card>
          <Card title="Contents" padded={false}>
            <ul className="divide-y divide-line">
              {inOu.users.map((u) => <Item key={u.id} to={`/users?id=${u.id}`} icon={<Avatar name={u.display_name} className="size-7" />} title={u.display_name} sub={`User · ${u.username}`} />)}
              {inOu.groups.map((g) => <Item key={g.id} to={`/groups?id=${g.id}`} icon={<span className="grid size-7 place-items-center rounded-lg bg-accent-soft text-accent"><UsersRound className="size-3.5" /></span>} title={g.name} sub={`Security group · ${g.member_ids.length} members`} />)}
              {inOu.endpoints.map((e) => <Item key={e.id} to={`/endpoints/${e.id}`} icon={<span className="grid size-7 place-items-center rounded-lg bg-panel-2"><OsIcon os={e.os} /></span>} title={e.hostname} sub={`Endpoint · ${e.inventory.os_name}`} />)}
              {inOu.users.length + inOu.groups.length + inOu.endpoints.length === 0 && <li className="px-4 py-8 text-center text-sm text-ink-3">This OU is empty.</li>}
            </ul>
          </Card>
          <Explain term="ou" />
        </div>
      </div>
      <OuDialog value={editing} ous={all} defaultParent={sel} onClose={() => setEditing(null)} onSaved={(o) => { void ous.reload(); setEditing(null); setParams({ id: o.id }); }} />
      <Confirm open={confirm} onClose={() => setConfirm(false)} danger title={`Delete ${current?.name}?`} confirmLabel="Delete OU" body="Only empty OUs can be deleted. Move users, groups, endpoints and child OUs out first."
        onConfirm={async () => { try { await del(`/ous/${sel}`); toast("good", "OU deleted"); setParams({}); void ous.reload(); } catch (e) { toast("bad", (e as Error).message); } }} />
    </>
  );
}

function Count({ icon, label, n }: { icon: React.ReactNode; label: string; n: number }) {
  return <div className="flex items-center gap-3 rounded-lg bg-panel-2 px-3 py-2.5"><span className="text-ink-3">{icon}</span><span className="flex-1 text-sm text-ink-2">{label}</span><span className="text-lg font-semibold">{n}</span></div>;
}
function Item({ to, icon, title, sub }: { to: string; icon: React.ReactNode; title: string; sub: string }) {
  return <li><Link to={to} className="flex items-center gap-3 px-4 py-2.5 hover:bg-panel-2">{icon}<div><div className="text-sm font-medium">{title}</div><div className="text-xs text-ink-3">{sub}</div></div></Link></li>;
}

function OuDialog({ value, ous, defaultParent, onClose, onSaved }: { value: OrgUnit | "new" | null; ous: OrgUnit[]; defaultParent: string | null; onClose: () => void; onSaved: (o: OrgUnit) => void }) {
  const toast = useToast();
  const isNew = value === "new";
  const [name, setName] = useState("");
  const [desc, setDesc] = useState("");
  const [parent, setParent] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [lastKey, setLastKey] = useState<string | null>(null);
  const key = value === null ? null : isNew ? "new" : value.id;
  if (key !== lastKey) {
    setLastKey(key);
    if (value && value !== "new") { setName(value.name); setDesc(value.description); setParent(value.parent_id); }
    else { setName(""); setDesc(""); setParent(defaultParent); }
  }
  return (
    <Dialog open={value !== null} onClose={onClose} title={isNew ? "New organizational unit" : "Edit organizational unit"}
      footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" loading={busy} disabled={!name.trim()} onClick={async () => {
        setBusy(true);
        try {
          const o = isNew ? await post<OrgUnit>("/ous", { name, description: desc, parent_id: parent }) : await patch<OrgUnit>(`/ous/${(value as OrgUnit).id}`, { name, description: desc, parent_id: parent });
          toast("good", isNew ? `Created ${o.name}` : "Saved"); onSaved(o);
        } catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
      }}>{isNew ? "Create OU" : "Save"}</Button></>}>
      <Field label="Name"><Input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Sales" /></Field>
      <Field label="Description"><Textarea rows={2} value={desc} onChange={(e) => setDesc(e.target.value)} /></Field>
      <Field label="Parent OU" hint="Child OUs inherit GPOs linked to their parent."><OuSelect ous={ous.filter((o) => isNew || o.id !== (value as OrgUnit | null)?.id)} value={parent} onChange={setParent} /></Field>
    </Dialog>
  );
}
