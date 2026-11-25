// Recycle Bin: restore deleted users, groups, OUs, GPOs and scripts.
import { Folder, RotateCcw, ScrollText, Terminal, Trash2, User as UserIcon, UsersRound } from "lucide-react";
import { useState } from "react";
import { useQuery } from "../lib/hooks";
import { del, post } from "../lib/api";
import { ago, dateTime } from "../lib/format";
import { Badge, Button, Card, Confirm, EmptyState, Explain, Loading, PageHeader, useToast } from "../components/ui";

interface Entry { id: string; kind: "user" | "group" | "ou" | "policy" | "script"; name: string; deleted_by: string; deleted_at: string; purge_at: string }
const KIND: Record<Entry["kind"], { label: string; icon: React.ReactNode }> = {
  user: { label: "User", icon: <UserIcon className="size-4" /> }, group: { label: "Security group", icon: <UsersRound className="size-4" /> },
  ou: { label: "OU", icon: <Folder className="size-4" /> }, policy: { label: "GPO", icon: <ScrollText className="size-4" /> }, script: { label: "Script", icon: <Terminal className="size-4" /> },
};

export default function RecycleBin() {
  const list = useQuery<Entry[]>("/recycle-bin");
  const toast = useToast();
  const [purge, setPurge] = useState<Entry | null>(null);
  return (
    <>
      <PageHeader title="Recycle Bin" term="recycle_bin" subtitle="Deleted objects are kept for 180 days with their attributes, group memberships and passwords, so you can restore them exactly." />
      <Card padded={false}>
        {list.loading && !list.data ? <Loading /> : (list.data ?? []).length === 0 ? <EmptyState icon={<Trash2 className="size-5" />} title="The Recycle Bin is empty" body="When you delete a user, group, OU, GPO or script, it appears here." /> : (
          <ul className="divide-y divide-line">
            {list.data!.map((e) => (
              <li key={e.id} className="flex flex-wrap items-center gap-3 px-4 py-3">
                <span className="grid size-8 place-items-center rounded-lg bg-panel-2 text-ink-2">{KIND[e.kind].icon}</span>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2"><span className="font-medium">{e.name}</span><Badge>{KIND[e.kind].label}</Badge></div>
                  <div className="text-xs text-ink-3">Deleted by {e.deleted_by} {ago(e.deleted_at)} · removed for good on {dateTime(e.purge_at)}</div>
                </div>
                <Button size="sm" variant="primary" icon={<RotateCcw className="size-3.5" />} onClick={async () => {
                  try { const r = await post<{ message: string; warnings: string[] }>(`/recycle-bin/${e.id}/restore`); toast("good", [r.message, ...r.warnings].join(" ")); void list.reload(); } catch (err) { toast("bad", (err as Error).message); }
                }}>Restore</Button>
                <Button size="sm" variant="ghost" icon={<Trash2 className="size-3.5" />} onClick={() => setPurge(e)} aria-label="Delete permanently" />
              </li>
            ))}
          </ul>
        )}
      </Card>
      <div className="mt-5"><Explain>Restore a group before its members to bring back their memberships. Endpoints aren't kept here: retiring one revokes its credentials, so re-enroll it instead.</Explain></div>
      <Confirm open={!!purge} onClose={() => setPurge(null)} danger title={`Permanently delete ${purge?.name}?`} confirmLabel="Delete permanently" body="It can't be restored afterwards."
        onConfirm={async () => { if (purge) { await del(`/recycle-bin/${purge.id}`); void list.reload(); } }} />
    </>
  );
}
