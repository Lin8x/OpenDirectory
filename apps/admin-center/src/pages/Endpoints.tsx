import { useMemo, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { Monitor, PlusCircle, Search } from "lucide-react";
import { useQuery } from "../lib/hooks";
import type { Endpoint, OrgUnit } from "../lib/types";
import { ago } from "../lib/format";
import { Badge, Button, Card, EmptyState, ErrorNote, Explain, Input, Loading, OsIcon, PageHeader, Row, StatusDot, Table, Td } from "../components/ui";
import { ouPath } from "./Users";

export function complianceSummary(e: Endpoint) {
  const bad = e.compliance.filter((c) => c.state === "non_compliant" || c.state === "failed").length;
  if (!e.compliance.length) return <Badge>Pending</Badge>;
  return bad ? <Badge tone="warn">{bad} issue{bad > 1 ? "s" : ""}</Badge> : <Badge tone="good">Compliant</Badge>;
}

export default function Endpoints() {
  const eps = useQuery<Endpoint[]>("/endpoints", { poll: 10000 });
  const ous = useQuery<OrgUnit[]>("/ous");
  const nav = useNavigate();
  const [params] = useSearchParams();
  const [q, setQ] = useState("");
  const [os, setOs] = useState<"all" | "windows" | "macos" | "linux">("all");
  const [onlyIssues, setOnlyIssues] = useState(params.get("filter") === "noncompliant");

  const list = useMemo(() => {
    const n = q.trim().toLowerCase();
    return (eps.data ?? []).filter((e) => {
      const hay = [e.hostname, e.inventory.serial_number, e.inventory.model, e.inventory.os_name, e.public_ip, ...e.inventory.interfaces.flatMap((i) => [i.mac, ...i.ips]), ...e.inventory.logged_in_users];
      const issues = e.compliance.some((c) => c.state === "non_compliant" || c.state === "failed");
      return (!n || hay.some((h) => h.toLowerCase().includes(n))) && (os === "all" || e.os === os) && (!onlyIssues || issues);
    });
  }, [eps.data, q, os, onlyIssues]);

  return (
    <>
      <PageHeader title="Endpoints" term="endpoint" subtitle="Every managed computer, its live status, hardware and policy compliance."
        actions={<Button variant="primary" icon={<PlusCircle className="size-4" />} onClick={() => nav("/enroll")}>Enroll endpoint</Button>} />
      <Card padded={false}>
        <div className="flex flex-wrap items-center gap-2 border-b border-line p-3">
          <div className="relative w-80"><Search className="pointer-events-none absolute left-2.5 top-2 size-4 text-ink-3" /><Input className="pl-8" placeholder="Hostname, serial number, IP, MAC or user" value={q} onChange={(e) => setQ(e.target.value)} /></div>
          <div className="flex gap-1">
            {(["all", "windows", "macos", "linux"] as const).map((o) => (
              <Button key={o} size="sm" variant={os === o ? "primary" : "ghost"} onClick={() => setOs(o)} icon={o !== "all" ? <OsIcon os={o} className="size-3.5" /> : undefined}>
                {o === "all" ? "All" : o === "macos" ? "macOS" : o[0]!.toUpperCase() + o.slice(1)}
              </Button>
            ))}
          </div>
          <Button size="sm" variant={onlyIssues ? "primary" : "ghost"} onClick={() => setOnlyIssues(!onlyIssues)}>Needs attention</Button>
          <span className="ml-auto text-xs text-ink-3">{list.length} of {eps.data?.length ?? 0}</span>
        </div>
        {eps.loading && !eps.data ? <Loading /> : eps.error ? <div className="p-4"><ErrorNote message={eps.error} onRetry={eps.reload} /></div> : (
          <Table head={["Endpoint", "Operating system", "Serial number", "IP address", "OU", "Compliance", "Last seen"]}
            empty={list.length === 0 && <EmptyState icon={<Monitor className="size-5" />} title={eps.data?.length ? "No matching endpoints" : "No endpoints enrolled yet"}
              body="Install the Endpoint Agent on a Windows, macOS or Linux computer with one command." action={<Button variant="primary" onClick={() => nav("/enroll")}>Enroll your first endpoint</Button>} />}>
            {list.map((e) => (
              <Row key={e.id} onClick={() => nav(`/endpoints/${e.id}`)}>
                <Td><div className="flex items-center gap-2.5"><StatusDot online={e.online} /><span className="grid size-8 place-items-center rounded-lg bg-panel-2"><OsIcon os={e.os} /></span>
                  <div><div className="font-medium">{e.hostname}</div><div className="text-xs text-ink-3">{[e.inventory.manufacturer, e.inventory.model].filter(Boolean).join(" ") || "—"}</div></div></div></Td>
                <Td><div className="text-ink-2">{e.inventory.os_name || e.os}</div><div className="text-xs text-ink-3">{e.inventory.os_edition || e.inventory.os_version}</div></Td>
                <Td className="font-mono text-[12.5px] text-ink-2">{e.inventory.serial_number || "—"}</Td>
                <Td className="font-mono text-[12.5px] text-ink-2">{e.inventory.interfaces[0]?.ips.find((i) => !i.includes(":")) ?? e.public_ip}</Td>
                <Td className="text-ink-2">{ouPath(ous.data ?? [], e.ou_id)}</Td>
                <Td>{complianceSummary(e)}</Td>
                <Td className="text-ink-3">{e.online ? <span className="text-good">Online</span> : ago(e.last_seen_at)}</Td>
              </Row>
            ))}
          </Table>
        )}
      </Card>
      <div className="mt-5"><Explain term="agent" /></div>
    </>
  );
}
