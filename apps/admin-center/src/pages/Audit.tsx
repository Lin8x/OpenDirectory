import { useMemo, useRef, useState } from "react";
import { Download, Search, ScrollText } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { saveExport } from "../lib/api";
import type { AuditEntry } from "../lib/types";
import { dateTime } from "../lib/format";
import { Badge, Button, Card, EmptyState, ErrorNote, Input, Loading, PageHeader, Row, Table, Td, useToast } from "../components/ui";
import { describe } from "./Overview";

type Category = "all" | "changes" | "signins" | "failed" | "endpoints";
const CATEGORIES: { id: Category; label: string; match: (a: AuditEntry) => boolean }[] = [
  { id: "all", label: "All", match: () => true },
  { id: "changes", label: "Directory changes", match: (a) => !a.action.startsWith("auth.") && !a.action.startsWith("endpoint.") && !a.action.startsWith("job.") },
  { id: "endpoints", label: "Endpoints & scripts", match: (a) => a.action.startsWith("endpoint.") || a.action.startsWith("job.") },
  { id: "signins", label: "Sign-ins", match: (a) => a.action === "auth.login" },
  { id: "failed", label: "Failed sign-ins", match: (a) => a.action === "auth.failed" },
];

const csvCell = (v: string) => (/[",\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v);

export default function Audit() {
  const toast = useToast();
  const [q, setQ] = useState("");
  const [debounced, setDebounced] = useState("");
  const [cat, setCat] = useState<Category>("all");
  const timer = useRef<number | undefined>(undefined);
  const log = useQuery<AuditEntry[]>(`/audit?limit=1000${debounced ? `&q=${encodeURIComponent(debounced)}` : ""}`, { poll: 15000 });
  const rows = useMemo(() => (log.data ?? []).filter(CATEGORIES.find((c) => c.id === cat)!.match), [log.data, cat]);
  const failedCount = useMemo(() => (log.data ?? []).filter((a) => a.action === "auth.failed").length, [log.data]);

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
        <div className="flex flex-wrap items-center gap-2 border-b border-line p-3">
          <div className="relative w-72"><Search className="pointer-events-none absolute left-2.5 top-2 size-4 text-ink-3" />
            <Input className="pl-8" placeholder="Filter by user, action or object" value={q} onChange={(e) => {
              setQ(e.target.value);
              window.clearTimeout(timer.current);
              const v = e.target.value;
              timer.current = window.setTimeout(() => setDebounced(v), 250);
            }} />
          </div>
          {CATEGORIES.map((c) => (
            <Button key={c.id} size="sm" variant={cat === c.id ? "primary" : "ghost"} onClick={() => setCat(c.id)}>
              {c.label}{c.id === "failed" && failedCount > 0 && <span className={`ml-1 rounded-full px-1.5 text-[11px] ${cat === c.id ? "bg-white/25" : "bg-bad-soft text-bad"}`}>{failedCount}</span>}
            </Button>
          ))}
          <span className="ml-auto text-xs text-ink-3">{rows.length} {rows.length === 1 ? "entry" : "entries"}</span>
        </div>
        {log.loading && !log.data ? <Loading /> : log.error ? <div className="p-4"><ErrorNote message={log.error} onRetry={log.reload} /></div> : (
          <Table head={["When", "Who", "Action", "Object", "Details"]} empty={rows.length === 0 && <EmptyState icon={<ScrollText className="size-5" />} title="No entries" body="Nothing matches this filter." />}>
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
