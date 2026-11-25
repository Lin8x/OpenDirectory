import { useState } from "react";
import { Search, ScrollText } from "lucide-react";
import { useQuery } from "../lib/hooks";
import type { AuditEntry } from "../lib/types";
import { dateTime } from "../lib/format";
import { Badge, Card, EmptyState, Input, Loading, PageHeader, Row, Table, Td } from "../components/ui";
import { describe } from "./Overview";

export default function Audit() {
  const [q, setQ] = useState("");
  const [debounced, setDebounced] = useState("");
  const log = useQuery<AuditEntry[]>(`/audit?limit=500${debounced ? `&q=${encodeURIComponent(debounced)}` : ""}`, { poll: 15000 });
  return (
    <>
      <PageHeader title="Audit Log" subtitle="Every change and sign-in, who made it and when. Entries can't be edited or deleted from the Admin Center." />
      <Card padded={false}>
        <div className="border-b border-line p-3">
          <div className="relative w-80"><Search className="pointer-events-none absolute left-2.5 top-2 size-4 text-ink-3" />
            <Input className="pl-8" placeholder="Filter by user, action or object" value={q} onChange={(e) => { setQ(e.target.value); clearTimeout((window as unknown as { _t?: number })._t); (window as unknown as { _t?: number })._t = window.setTimeout(() => setDebounced(e.target.value), 250); }} />
          </div>
        </div>
        {log.loading && !log.data ? <Loading /> : (
          <Table head={["When", "Who", "Action", "Object", "Details"]} empty={(log.data ?? []).length === 0 && <EmptyState icon={<ScrollText className="size-5" />} title="No entries" body="Nothing matches this filter." />}>
            {(log.data ?? []).map((a) => (
              <Row key={a.id}>
                <Td className="whitespace-nowrap text-ink-3">{dateTime(a.at)}</Td>
                <Td className="font-medium">{a.actor}</Td>
                <Td><Badge tone={a.action.includes("failed") ? "bad" : a.action.includes("delete") || a.action.includes("retire") ? "warn" : "neutral"}>{describe(a.action)}</Badge></Td>
                <Td>{a.target}</Td>
                <Td className="text-ink-2">{a.details}</Td>
              </Row>
            ))}
          </Table>
        )}
      </Card>
    </>
  );
}
