import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { Monitor, Plus, Trash2, UserMinus, UsersRound } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { del, patch, post, put } from "../lib/api";
import type { Endpoint, Group, OrgUnit, User } from "../lib/types";
import { Avatar, Badge, OsIcon, Select, Button, Card, Confirm, Dialog, Drawer, EmptyState, ErrorNote, Explain, Field, Input, Loading, PageHeader, Row, Table, Td, Textarea, useToast } from "../components/ui";
import { OuSelect, ouFilter, ouPath } from "./Users";
import { ListToolbar, NoMatches, useListView } from "../components/DataView";

const BUILTIN = ["Domain Admins", "Domain Users"];

export default function Groups() {
  const groups = useQuery<Group[]>("/groups");
  const endpoints = useQuery<Endpoint[]>("/endpoints");
  const users = useQuery<User[]>("/users");
  const ous = useQuery<OrgUnit[]>("/ous");
  const [params, setParams] = useSearchParams();
  const [creating, setCreating] = useState(params.get("new") === "1");
  const ouList = ous.data ?? [];
  const view = useListView(groups.data, {
    id: "groups",
    search: (g) => [g.name, g.description],
    filters: [
      { kind: "enum", key: "type", label: "Type", get: (g) => (BUILTIN.includes(g.name) ? "builtin" : "custom"), options: [{ value: "builtin", label: "Built-in" }, { value: "custom", label: "Custom" }] },
      { kind: "enum", key: "members", label: "Members", get: (g) => [g.member_ids.length ? "users" : "nousers", g.endpoint_ids.length ? "computers" : "nocomputers"],
        options: [{ value: "users", label: "Has users" }, { value: "nousers", label: "No users" }, { value: "computers", label: "Has computers" }, { value: "nocomputers", label: "No computers" }] },
      ouFilter(ouList, (g: Group) => g.ou_id),
      { kind: "date", key: "created", label: "Created", get: (g) => g.created_at },
    ],
    sorts: {
      name: { label: "Name", get: (g) => g.name },
      description: { label: "Description", get: (g) => g.description },
      members: { label: "Members", get: (g) => g.member_ids.length, type: "number" },
      computers: { label: "Computers", get: (g) => g.endpoint_ids.length, type: "number" },
      ou: { label: "OU", get: (g) => (g.ou_id ? ouPath(ouList, g.ou_id) : null) },
    },
    defaultSort: { key: "name", dir: "asc" },
  });
  const list = view.rows;
  const selected = groups.data?.find((g) => g.id === params.get("id"));

  return (
    <>
      <PageHeader title="Security Groups" guide="02-directory#security-groups-teams-you-can-point-at" term="group" subtitle="Named sets of users. Target groups in Group Policy to decide who can sign in to which endpoints, and who is a local admin."
        actions={<Button variant="primary" icon={<Plus className="size-4" />} onClick={() => setCreating(true)}>New group</Button>} />
      <Card padded={false}>
        <ListToolbar view={view} placeholder="Search groups" noun={["group", "groups"]} />
        {groups.loading && !groups.data ? <Loading /> : groups.error ? <div className="p-4"><ErrorNote message={groups.error} onRetry={groups.reload} /></div> : (
          <Table view={view} head={[{ label: "Name", sort: "name" }, { label: "Description", sort: "description" }, { label: "Members", sort: "members" }, { label: "Computers", sort: "computers" }, { label: "OU", sort: "ou" }]}
            empty={list.length === 0 && (view.total > 0 ? <NoMatches view={view} /> : <EmptyState icon={<UsersRound className="size-5" />} title="No groups" body="Create a group such as Laptop Users or Finance." action={<Button variant="primary" onClick={() => setCreating(true)}>New group</Button>} />)}>
            {list.map((g) => (
              <Row key={g.id} onClick={() => setParams({ id: g.id })} selected={g.id === selected?.id}>
                <Td><div className="flex items-center gap-2.5"><span className="grid size-8 place-items-center rounded-lg bg-accent-soft text-accent"><UsersRound className="size-4" /></span><span className="font-medium">{g.name}</span>{BUILTIN.includes(g.name) && <Badge>Built-in</Badge>}</div></Td>
                <Td className="text-ink-2">{g.description || "—"}</Td>
                <Td><MemberStack ids={g.member_ids} users={users.data ?? []} /></Td>
                <Td className="text-ink-2">{g.endpoint_ids.length || "—"}</Td>
                <Td className="text-ink-2">{ouPath(ous.data ?? [], g.ou_id)}</Td>
              </Row>
            ))}
          </Table>
        )}
      </Card>
      <CreateGroup open={creating} ous={ous.data ?? []} onClose={() => { setCreating(false); setParams({}); }} onCreated={(g) => { void groups.reload(); setCreating(false); setParams({ id: g.id }); }} />
      {selected && <GroupDrawer group={selected} endpoints={endpoints.data ?? []} users={users.data ?? []} ous={ous.data ?? []} onClose={() => setParams({})} onChanged={() => { void groups.reload(); void users.reload(); }} />}
    </>
  );
}

function MemberStack({ ids, users }: { ids: string[]; users: User[] }) {
  const m = ids.map((id) => users.find((u) => u.id === id)).filter(Boolean) as User[];
  if (!m.length) return <span className="text-ink-3">None</span>;
  return (
    <div className="flex items-center">
      {m.slice(0, 4).map((u) => <Avatar key={u.id} name={u.display_name} className="-ml-1.5 size-7 ring-2 ring-panel first:ml-0" />)}
      <span className="ml-2 text-xs text-ink-3">{m.length}</span>
    </div>
  );
}

function CreateGroup({ open, ous, onClose, onCreated }: { open: boolean; ous: OrgUnit[]; onClose: () => void; onCreated: (g: Group) => void }) {
  const toast = useToast();
  const [name, setName] = useState("");
  const [desc, setDesc] = useState("");
  const [ou, setOu] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { if (open) { setName(""); setDesc(""); setOu(null); } }, [open]);
  return (
    <Dialog open={open} onClose={onClose} title="New security group"
      footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" loading={busy} disabled={!name.trim()} onClick={async () => {
        setBusy(true);
        try { const g = await post<Group>("/groups", { name, description: desc, ou_id: ou }); toast("good", `Created ${g.name}`); onCreated(g); } catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
      }}>Create group</Button></>}>
      <Explain term="group" />
      <Field label="Name"><Input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Laptop Users" /></Field>
      <Field label="Description"><Textarea rows={2} value={desc} onChange={(e) => setDesc(e.target.value)} placeholder="Staff who sign in to company laptops" /></Field>
      <Field label="Organizational unit" term="ou"><OuSelect ous={ous} value={ou} onChange={setOu} /></Field>
    </Dialog>
  );
}

function GroupDrawer({ group, endpoints, users, ous, onClose, onChanged }: { group: Group; endpoints: Endpoint[]; users: User[]; ous: OrgUnit[]; onClose: () => void; onChanged: () => void }) {
  const toast = useToast();
  const [name, setName] = useState(group.name);
  const [desc, setDesc] = useState(group.description);
  const [ou, setOu] = useState(group.ou_id);
  const [adding, setAdding] = useState("");
  const [confirm, setConfirm] = useState(false);
  useEffect(() => { setName(group.name); setDesc(group.description); setOu(group.ou_id); }, [group]);
  const builtin = BUILTIN.includes(group.name);
  const members = users.filter((u) => group.member_ids.includes(u.id));
  const candidates = users.filter((u) => !group.member_ids.includes(u.id) && (!adding || `${u.display_name} ${u.username}`.toLowerCase().includes(adding.toLowerCase()))).slice(0, 6);
  const setMembers = async (ids: string[], msg: string) => {
    try { await put(`/groups/${group.id}/members`, { ids }); toast("good", msg); onChanged(); } catch (e) { toast("bad", (e as Error).message); }
  };
  const dirty = name !== group.name || desc !== group.description || ou !== group.ou_id;

  return (
    <Drawer open onClose={onClose} title={group.name} subtitle={`${members.length} member${members.length === 1 ? "" : "s"}`}
      actions={!builtin && <Button size="sm" variant="ghost" icon={<Trash2 className="size-3.5" />} onClick={() => setConfirm(true)}>Delete</Button>}>
      <div className="space-y-4">
        <Field label="Name"><Input value={name} onChange={(e) => setName(e.target.value)} disabled={builtin} /></Field>
        <Field label="Description"><Textarea rows={2} value={desc} onChange={(e) => setDesc(e.target.value)} /></Field>
        <Field label="Organizational unit"><OuSelect ous={ous} value={ou} onChange={setOu} /></Field>
        <div className="flex justify-end"><Button variant="primary" disabled={!dirty} onClick={async () => {
          try { await patch(`/groups/${group.id}`, { name, description: desc, ou_id: ou }); toast("good", "Saved"); onChanged(); } catch (e) { toast("bad", (e as Error).message); }
        }}>Save</Button></div>
      </div>
      <h3 className="mb-2 mt-6 text-sm font-semibold">Members</h3>
      <div className="relative mb-3">
        <Input placeholder="Add a member: type a name or username" value={adding} onChange={(e) => setAdding(e.target.value)} />
        {adding && (
          <div className="absolute z-10 mt-1 w-full rounded-lg border border-line bg-panel p-1 shadow-lg">
            {candidates.length === 0 && <div className="px-3 py-2 text-sm text-ink-3">No users found</div>}
            {candidates.map((u) => (
              <button key={u.id} onClick={() => { setAdding(""); void setMembers([...group.member_ids, u.id], `Added ${u.display_name}`); }} className="flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-sm hover:bg-panel-2">
                <Avatar name={u.display_name} className="size-6" />{u.display_name}<span className="text-xs text-ink-3">{u.username}</span>
              </button>
            ))}
          </div>
        )}
      </div>
      <div className="divide-y divide-line rounded-lg border border-line">
        {members.length === 0 && <div className="px-3 py-6 text-center text-sm text-ink-3">No members yet</div>}
        {members.map((u) => (
          <div key={u.id} className="flex items-center gap-3 px-3 py-2">
            <Avatar name={u.display_name} className="size-7" />
            <div className="flex-1"><div className="text-sm font-medium">{u.display_name}</div><div className="text-xs text-ink-3">{u.username}</div></div>
            {!u.enabled && <Badge>Disabled</Badge>}
            <Button size="sm" variant="ghost" icon={<UserMinus className="size-3.5" />} onClick={() => void setMembers(group.member_ids.filter((x) => x !== u.id), `Removed ${u.display_name}`)}>Remove</Button>
          </div>
        ))}
      </div>
      <h3 className="mb-1 mt-6 text-sm font-semibold">Computers</h3>
      <p className="mb-2 text-xs text-ink-3">Endpoints in this group, like computer accounts in an AD group. Run library scripts against the group to reach all of them.</p>
      <ComputerMembers group={group} endpoints={endpoints} onChanged={onChanged} />
      <Confirm open={confirm} onClose={() => setConfirm(false)} danger title={`Delete ${group.name}?`} confirmLabel="Delete group"
        body="Members keep their accounts. GPOs that use this group for sign-in accounts must be changed first."
        onConfirm={async () => { try { await del(`/groups/${group.id}`); toast("good", "Group deleted"); onChanged(); onClose(); } catch (e) { toast("bad", (e as Error).message); } }} />
    </Drawer>
  );
}

function ComputerMembers({ group, endpoints, onChanged }: { group: Group; endpoints: Endpoint[]; onChanged: () => void }) {
  const toast = useToast();
  const [pick, setPick] = useState("");
  const members = endpoints.filter((e) => group.endpoint_ids.includes(e.id));
  const save = async (ids: string[], msg: string) => {
    try { await put(`/groups/${group.id}/endpoints`, { ids }); toast("good", msg); onChanged(); } catch (e) { toast("bad", (e as Error).message); }
  };
  return (
    <>
      <div className="mb-3 flex gap-2">
        <Select value={pick} onChange={(e) => setPick(e.target.value)}>
          <option value="">Add a computer…</option>
          {endpoints.filter((e) => !group.endpoint_ids.includes(e.id)).map((e) => <option key={e.id} value={e.id}>{e.hostname}</option>)}
        </Select>
        <Button disabled={!pick} onClick={() => { const e = endpoints.find((x) => x.id === pick); setPick(""); void save([...group.endpoint_ids, pick], `Added ${e?.hostname}`); }}>Add</Button>
      </div>
      <div className="divide-y divide-line rounded-lg border border-line">
        {members.length === 0 && <div className="px-3 py-6 text-center text-sm text-ink-3">No computers yet</div>}
        {members.map((e) => (
          <div key={e.id} className="flex items-center gap-3 px-3 py-2">
            <span className="grid size-7 place-items-center rounded-md bg-panel-2"><OsIcon os={e.os} className="size-3.5" /></span>
            <div className="flex-1 text-sm font-medium">{e.hostname}</div>
            <Button size="sm" variant="ghost" icon={<Monitor className="size-3.5" />} onClick={() => void save(group.endpoint_ids.filter((x) => x !== e.id), `Removed ${e.hostname}`)}>Remove</Button>
          </div>
        ))}
      </div>
    </>
  );
}
