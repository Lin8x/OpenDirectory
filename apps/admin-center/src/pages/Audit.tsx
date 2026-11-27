import { useEffect, useMemo, useRef, useState } from "react";
import { Download, ScrollText } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { saveExport } from "../lib/api";
import type { AuditEntry } from "../lib/types";
import { dateTime } from "../lib/format";
import { Badge, Button, Card, EmptyState, ErrorNote, Loading, PageHeader, Row, Table, Td, useToast } from "../components/ui";
import { describe } from "./Overview";
import { ListToolbar, NoMatches, useListView } from "../components/DataView";

const CATEGORIES: { id: string; label: string; match: (a: AuditEntry) => boolean }[] = [
  { id: "changes", label: "Directory changes", match: (a) => !a.action.startsWith("auth.") && !a.action.startsWith("endpoint.") && !a.action.startsWith("job.") },
  { id: "endpoints", label: "Endpoints & scripts", match: (a) => a.action.startsWith("endpoint.") || a.action.startsWith("job.") },
  { id: "signins", label: "Sign-ins", match: (a) => a.action === "auth.login" },
  { id: "failed", label: "Failed sign-ins", match: (a) => a.action === "auth.failed" },
];

const csvCell = (v: string) => (/[",\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v);

export default function Audit() {
  const toast = useToast();
  const [debounced, setDebounced] = useState("");
  const timer = useRef<number | undefined>(undefined);
  // Search runs on the server too, so it reaches entries past the newest 1000.
  const log = useQuery<AuditEntry[]>(`/audit?limit=1000${debounced ? `&q=${encodeURIComponent(debounced)}` : ""}`, { poll: 15000 });
  const failedCount = useMemo(() => (log.data ?? []).filter((a) => a.action === "auth.failed").length, [log.data]);
  const view = useListView(log.data, {
    id: "audit",
    search: (a) => [a.actor, a.action, describe(a.action), a.target, a.details],
    filters: [
      { kind: "enum", key: "category", label: "Category", get: (a) => CATEGORIES.filter((c) => c.match(a)).map((c) => c.id),
        options: CATEGORIES.map((c) => ({ value: c.id, label: c.id === "failed" && failedCount ? <span className="flex items-center gap-1.5">{c.label}<Badge tone="bad" className="py-0">{failedCount}</Badge></span> : c.label })) },
      { kind: "enum", key: "actor", label: "Who", get: (a) => a.actor },
      { kind: "enum", key: "action", label: "Action", get: (a) => a.action, format: describe },
      { kind: "date", key: "at", label: "When", get: (a) => a.at },
    ],
    sorts: {
      at: { label: "When", get: (a) => a.at, type: "date" },
      actor: { label: "Who", get: (a) => a.actor },
      action: { label: "Action", get: (a) => describe(a.action) },
      target: { label: "Object", get: (a) => a.target },
    },
    defaultSort: { key: "at", dir: "desc" },
  });
  useEffect(() => {
    window.clearTimeout(timer.current);
    const v = view.q.trim();
    timer.current = window.setTimeout(() => setDebounced(v), 250);
    return () => window.clearTimeout(timer.current);
  }, [view.q]);
  const rows = view.rows;

  const exportCsv = async () => {
    const lines = [["Time (UTC)", "Who", "Action", "Object", "Details"].join(",")];
    for (const a of rows) lines.push([a.at, a.actor, describe(a.action), a.target, a.details].map((v) => csvCell(v ?? "")).join(","));
    try {
      const path = await saveExport(`opendirectory-audit-${new Date().toISOString().slice(0, 10)}.csv`, lines.join("\n") + "\n");
      toast("good", path ? `Saved ${rows.length} entries to ${path}` : `Exported ${rows.length} entries`);
    } catch (e) {
      toast("bad", `Export failed: ${(e as Error).message}`);
    }
  };

  return (
    <>
      <PageHeader
        title="Audit Log"
        subtitle="Every change and sign-in, who made it and when. Entries can't be edited or deleted from the Admin Center."
        actions={<Button icon={<Download className="size-4" />} onClick={exportCsv} disabled={rows.length === 0}>Export CSV</Button>}
      />
      <Card padded={false}>
        <ListToolbar view={view} placeholder="Filter by user, action or object" noun={["entry", "entries"]} />
        {log.loading && !log.data ? <Loading /> : log.error ? <div className="p-4"><ErrorNote message={log.error} onRetry={log.reload} /></div> : (
          <Table view={view} head={[{ label: "When", sort: "at" }, { label: "Who", sort: "actor" }, { label: "Action", sort: "action" }, { label: "Object", sort: "target" }, "Details"]}
            empty={rows.length === 0 && (view.activeCount ? <NoMatches view={view} /> : <EmptyState icon={<ScrollText className="size-5" />} title="No entries" body="Nothing has been recorded yet." />)}>
            {rows.map((a) => (
              <Row key={a.id}>
                <Td className="whitespace-nowrap text-ink-3">{dateTime(a.at)}</Td>
                <Td className="font-medium">{a.actor}</Td>
                <Td><Badge tone={a.action.includes("failed") ? "bad" : a.action.includes("delete") || a.action.includes("retire") ? "warn" : "neutral"}>{describe(a.action)}</Badge></Td>
                <Td>{a.target || <span className="text-ink-3">—</span>}</Td>
                <Td className="text-ink-2">{a.details}</Td>
              </Row>
            ))}
          </Table>
        )}
      </Card>
    </>
  );
}
