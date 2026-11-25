import { useEffect, useMemo, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { Plus, Search, Trash2, UserMinus, UsersRound } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { del, patch, post, put } from "../lib/api";
import type { Group, OrgUnit, User } from "../lib/types";
import { Avatar, Badge, Button, Card, Confirm, Dialog, Drawer, EmptyState, ErrorNote, Explain, Field, Input, Loading, PageHeader, Row, Table, Td, Textarea, useToast } from "../components/ui";
import { OuSelect, ouPath } from "./Users";

const BUILTIN = ["Domain Admins", "Domain Users"];

export default function Groups() {
  const groups = useQuery<Group[]>("/groups");
  const users = useQuery<User[]>("/users");
  const ous = useQuery<OrgUnit[]>("/ous");
  const [params, setParams] = useSearchParams();
  const [q, setQ] = useState("");
  const [creating, setCreating] = useState(params.get("new") === "1");
  const list = useMemo(() => (groups.data ?? []).filter((g) => !q || g.name.toLowerCase().includes(q.toLowerCase()) || g.description.toLowerCase().includes(q.toLowerCase())), [groups.data, q]);
  const selected = groups.data?.find((g) => g.id === params.get("id"));

  return (
    <>
      <PageHeader title="Security Groups" term="group" subtitle="Named sets of users. Target groups in Group Policy to decide who can sign in to which endpoints, and who is a local admin."
        actions={<Button variant="primary" icon={<Plus className="size-4" />} onClick={() => setCreating(true)}>New group</Button>} />
      <Card padded={false}>
        <div className="flex items-center gap-2 border-b border-line p-3">
          <div className="relative w-72"><Search className="pointer-events-none absolute left-2.5 top-2 size-4 text-ink-3" /><Input className="pl-8" placeholder="Search groups" value={q} onChange={(e) => setQ(e.target.value)} /></div>
        </div>
        {groups.loading && !groups.data ? <Loading /> : groups.error ? <div className="p-4"><ErrorNote message={groups.error} onRetry={groups.reload} /></div> : (
          <Table head={["Name", "Description", "Members", "OU"]}
            empty={list.length === 0 && <EmptyState icon={<UsersRound className="size-5" />} title="No groups" body="Create a group such as Laptop Users or Finance." action={<Button variant="primary" onClick={() => setCreating(true)}>New group</Button>} />}>
            {list.map((g) => (
              <Row key={g.id} onClick={() => setParams({ id: g.id })} selected={g.id === selected?.id}>
                <Td><div className="flex items-center gap-2.5"><span className="grid size-8 place-items-center rounded-lg bg-accent-soft text-accent"><UsersRound className="size-4" /></span><span className="font-medium">{g.name}</span>{BUILTIN.includes(g.name) && <Badge>Built-in</Badge>}</div></Td>
                <Td className="text-ink-2">{g.description || "—"}</Td>
                <Td><MemberStack ids={g.member_ids} users={users.data ?? []} /></Td>
                <Td className="text-ink-2">{ouPath(ous.data ?? [], g.ou_id)}</Td>
              </Row>
            ))}
          </Table>
        )}
      </Card>
      <CreateGroup open={creating} ous={ous.data ?? []} onClose={() => { setCreating(false); setParams({}); }} onCreated={(g) => { void groups.reload(); setCreating(false); setParams({ id: g.id }); }} />
      {selected && <GroupDrawer group={selected} users={users.data ?? []} ous={ous.data ?? []} onClose={() => setParams({})} onChanged={() => { void groups.reload(); void users.reload(); }} />}
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

function GroupDrawer({ group, users, ous, onClose, onChanged }: { group: Group; users: User[]; ous: OrgUnit[]; onClose: () => void; onChanged: () => void }) {
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
      <Confirm open={confirm} onClose={() => setConfirm(false)} danger title={`Delete ${group.name}?`} confirmLabel="Delete group"
        body="Members keep their accounts. GPOs that use this group for sign-in accounts must be changed first."
        onConfirm={async () => { try { await del(`/groups/${group.id}`); toast("good", "Group deleted"); onChanged(); onClose(); } catch (e) { toast("bad", (e as Error).message); } }} />
    </Drawer>
  );
}
