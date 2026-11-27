// Recycle Bin: restore deleted users, groups, OUs, GPOs and scripts.
import { Folder, RotateCcw, ScrollText, Terminal, Trash2, User as UserIcon, UsersRound } from "lucide-react";
import { useState } from "react";
import { useQuery } from "../lib/hooks";
import { del, post } from "../lib/api";
import { ago, dateTime } from "../lib/format";
import { Badge, Button, Card, Confirm, EmptyState, Explain, Loading, PageHeader, Row, Table, Td, useToast } from "../components/ui";
import { ListToolbar, NoMatches, useListView } from "../components/DataView";

interface Entry { id: string; kind: "user" | "group" | "ou" | "policy" | "script"; name: string; deleted_by: string; deleted_at: string; purge_at: string }
const KIND: Record<Entry["kind"], { label: string; icon: React.ReactNode }> = {
  user: { label: "User", icon: <UserIcon className="size-4" /> }, group: { label: "Security group", icon: <UsersRound className="size-4" /> },
  ou: { label: "OU", icon: <Folder className="size-4" /> }, policy: { label: "GPO", icon: <ScrollText className="size-4" /> }, script: { label: "Script", icon: <Terminal className="size-4" /> },
};

export default function RecycleBin() {
  const list = useQuery<Entry[]>("/recycle-bin");
  const toast = useToast();
  const [purge, setPurge] = useState<Entry | null>(null);
  const view = useListView(list.data, {
    id: "recycle-bin",
    search: (e) => [e.name, e.deleted_by, KIND[e.kind].label],
    filters: [
      { kind: "enum", key: "kind", label: "Object type", get: (e) => e.kind, options: (Object.keys(KIND) as Entry["kind"][]).map((k) => ({ value: k, label: KIND[k].label })) },
      { kind: "enum", key: "by", label: "Deleted by", get: (e) => e.deleted_by },
      { kind: "date", key: "deleted", label: "Deleted", get: (e) => e.deleted_at },
    ],
    sorts: {
      name: { label: "Name", get: (e) => e.name },
      kind: { label: "Object type", get: (e) => KIND[e.kind].label },
      by: { label: "Deleted by", get: (e) => e.deleted_by },
      deleted: { label: "Deleted", get: (e) => e.deleted_at, type: "date" },
      purge: { label: "Purged on", get: (e) => e.purge_at, type: "date" },
    },
    defaultSort: { key: "deleted", dir: "desc" },
  });
  return (
    <>
      <PageHeader title="Recycle Bin" term="recycle_bin" subtitle="Deleted objects are kept for 180 days with their attributes, group memberships and passwords, so you can restore them exactly." />
      <Card padded={false}>
        {list.loading && !list.data ? <Loading /> : (list.data ?? []).length === 0 ? <EmptyState icon={<Trash2 className="size-5" />} title="The Recycle Bin is empty" body="When you delete a user, group, OU, GPO or script, it appears here." /> : (
          <>
            <ListToolbar view={view} placeholder="Search name or who deleted it" noun={["object", "objects"]} />
            <Table view={view} head={[{ label: "Name", sort: "name" }, { label: "Object type", sort: "kind" }, { label: "Deleted by", sort: "by" }, { label: "Deleted", sort: "deleted" }, { label: "Purged on", sort: "purge" }, ""]}
              empty={view.rows.length === 0 && <NoMatches view={view} />}>
              {view.rows.map((e) => (
                <Row key={e.id}>
                  <Td><div className="flex items-center gap-2.5"><span className="grid size-8 place-items-center rounded-lg bg-panel-2 text-ink-2">{KIND[e.kind].icon}</span><span className="font-medium">{e.name}</span></div></Td>
                  <Td><Badge>{KIND[e.kind].label}</Badge></Td>
                  <Td className="text-ink-2">{e.deleted_by}</Td>
                  <Td className="text-ink-3" ><span title={dateTime(e.deleted_at)}>{ago(e.deleted_at)}</span></Td>
                  <Td className="whitespace-nowrap text-ink-3">{dateTime(e.purge_at)}</Td>
                  <Td>
                    <div className="flex justify-end gap-1">
                      <Button size="sm" variant="primary" icon={<RotateCcw className="size-3.5" />} onClick={async () => {
                        try { const r = await post<{ message: string; warnings: string[] }>(`/recycle-bin/${e.id}/restore`); toast("good", [r.message, ...r.warnings].join(" ")); void list.reload(); } catch (err) { toast("bad", (err as Error).message); }
                      }}>Restore</Button>
                      <Button size="sm" variant="ghost" icon={<Trash2 className="size-3.5" />} onClick={() => setPurge(e)} aria-label="Delete permanently" />
                    </div>
                  </Td>
                </Row>
              ))}
            </Table>
          </>
        )}
      </Card>
      <div className="mt-5"><Explain>Restore a group before its members to bring back their memberships. Endpoints aren't kept here: retiring one revokes its credentials, so re-enroll it instead.</Explain></div>
      <Confirm open={!!purge} onClose={() => setPurge(null)} danger title={`Permanently delete ${purge?.name}?`} confirmLabel="Delete permanently" body="It can't be restored afterwards."
        onConfirm={async () => { if (purge) { await del(`/recycle-bin/${purge.id}`); void list.reload(); } }} />
    </>
  );
}
