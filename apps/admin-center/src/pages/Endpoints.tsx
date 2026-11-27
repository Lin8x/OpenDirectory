import { useNavigate, useSearchParams } from "react-router-dom";
import { Monitor, PlusCircle } from "lucide-react";
import { useQuery } from "../lib/hooks";
import type { Endpoint, OrgUnit } from "../lib/types";
import { ago, osLabel } from "../lib/format";
import { Badge, Button, Card, EmptyState, ErrorNote, Explain, Loading, OsIcon, PageHeader, Row, StatusDot, Table, Td } from "../components/ui";
import { ouFilter, ouPath } from "./Users";
import { ipKey, ListToolbar, NoMatches, useListView } from "../components/DataView";

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
  const ouList = ous.data ?? [];
  const ipv4 = (e: Endpoint) => e.inventory.interfaces[0]?.ips.find((i) => !i.includes(":")) ?? e.public_ip;
  const issues = (e: Endpoint) => e.compliance.filter((c) => c.state === "non_compliant" || c.state === "failed").length;
  const view = useListView(eps.data, {
    id: "endpoints",
    search: (e) => [e.hostname, e.inventory.serial_number, e.inventory.model, e.inventory.manufacturer, e.inventory.os_name, e.public_ip, ...e.inventory.interfaces.flatMap((i) => [i.mac, ...i.ips]), ...e.inventory.logged_in_users],
    filters: [
      { kind: "enum", key: "os", label: "OS", get: (e) => e.os, options: (["windows", "macos", "linux", "android"] as const).map((o) => ({ value: o, label: osLabel[o], icon: <OsIcon os={o} className="size-3.5" /> })) },
      { kind: "enum", key: "status", label: "Status", get: (e) => (e.online ? "online" : "offline"), options: [{ value: "online", label: "Online" }, { value: "offline", label: "Offline" }] },
      { kind: "enum", key: "compliance", label: "Compliance", get: (e) => (!e.compliance.length ? "pending" : issues(e) ? "issues" : "compliant"), options: [{ value: "compliant", label: "Compliant" }, { value: "issues", label: "Non-compliant" }, { value: "pending", label: "Pending" }] },
      { kind: "enum", key: "version", label: "OS version", get: (e) => [e.inventory.os_name, e.inventory.os_version].filter(Boolean).join(" ") || null },
      ouFilter(ouList, (e: Endpoint) => e.ou_id),
      { kind: "date", key: "seen", label: "Last seen", get: (e) => (e.online ? new Date().toISOString() : e.last_seen_at) },
    ],
    sorts: {
      hostname: { label: "Endpoint", get: (e) => e.hostname },
      os: { label: "Operating system", get: (e) => `${e.inventory.os_name || e.os} ${e.inventory.os_version}` },
      serial: { label: "Serial number", get: (e) => e.inventory.serial_number },
      ip: { label: "IP address", get: (e) => ipKey(ipv4(e)) },
      ou: { label: "OU", get: (e) => (e.ou_id ? ouPath(ouList, e.ou_id) : null) },
      compliance: { label: "Compliance", get: (e) => (e.compliance.length ? issues(e) : -1), type: "number" },
      seen: { label: "Last seen", get: (e) => (e.online ? new Date(Date.now() + 1000).toISOString() : e.last_seen_at), type: "date" },
    },
    defaultSort: { key: "hostname", dir: "asc" },
    initial: params.get("filter") === "noncompliant" ? { compliance: ["issues"] } : undefined,
  });
  const list = view.rows;

  return (
    <>
      <PageHeader title="Endpoints" guide="04-endpoints-and-agents" term="endpoint" subtitle="Every managed computer, its live status, hardware and policy compliance."
        actions={<Button variant="primary" icon={<PlusCircle className="size-4" />} onClick={() => nav("/enroll")}>Enroll endpoint</Button>} />
      <Card padded={false}>
        <ListToolbar view={view} placeholder="Hostname, serial number, IP, MAC or user" noun={["endpoint", "endpoints"]} />
        {eps.loading && !eps.data ? <Loading /> : eps.error ? <div className="p-4"><ErrorNote message={eps.error} onRetry={eps.reload} /></div> : (
          <Table view={view} head={[{ label: "Endpoint", sort: "hostname" }, { label: "Operating system", sort: "os" }, { label: "Serial number", sort: "serial" }, { label: "IP address", sort: "ip" }, { label: "OU", sort: "ou" }, { label: "Compliance", sort: "compliance" }, { label: "Last seen", sort: "seen" }]}
            empty={list.length === 0 && (view.total > 0 ? <NoMatches view={view} /> : <EmptyState icon={<Monitor className="size-5" />} title={eps.data?.length ? "No matching endpoints" : "No endpoints enrolled yet"}
              body="Install the Endpoint Agent on a Windows, macOS or Linux computer with one command." action={<Button variant="primary" onClick={() => nav("/enroll")}>Enroll your first endpoint</Button>} />)}>
            {list.map((e) => (
              <Row key={e.id} onClick={() => nav(`/endpoints/${e.id}`)}>
                <Td><div className="flex items-center gap-2.5"><StatusDot online={e.online} /><span className="grid size-8 place-items-center rounded-lg bg-panel-2"><OsIcon os={e.os} /></span>
                  <div><div className="font-medium">{e.hostname}</div><div className="text-xs text-ink-3">{[e.inventory.manufacturer, e.inventory.model].filter(Boolean).join(" ") || "—"}</div></div></div></Td>
                <Td><div className="text-ink-2">{e.inventory.os_name || e.os}</div><div className="text-xs text-ink-3">{e.inventory.os_edition || e.inventory.os_version}</div></Td>
                <Td className="font-mono text-[12.5px] text-ink-2">{e.inventory.serial_number || "—"}</Td>
                <Td className="font-mono text-[12.5px] text-ink-2">{ipv4(e)}</Td>
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
