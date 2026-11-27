import { Link, useNavigate } from "react-router-dom";
import { Globe, Plus, ShieldCheck } from "lucide-react";
import { useQuery } from "../lib/hooks";
import type { OrgUnit, Policy } from "../lib/types";
import { ago } from "../lib/format";
import { Badge, Button, Card, EmptyState, Explain, Loading, PageHeader, Row, Table, Td } from "../components/ui";
import { ouPath } from "./Users";
import { ListToolbar, NoMatches, useListView } from "../components/DataView";

export default function Policies() {
  const ps = useQuery<Policy[]>("/policies");
  const ous = useQuery<OrgUnit[]>("/ous");
  const nav = useNavigate();
  const ouList = ous.data ?? [];
  const view = useListView(ps.data, {
    id: "gpos",
    search: (p) => [p.name, p.description, ...p.settings.map((x) => x.kind.replace(/_/g, " "))],
    filters: [
      { kind: "enum", key: "status", label: "Status", get: (p) => (p.enabled ? "enabled" : "disabled"), options: [{ value: "enabled", label: "Enabled" }, { value: "disabled", label: "Disabled" }] },
      { kind: "enum", key: "link", label: "Linked to", get: (p) => [...(p.targets.all_endpoints ? ["__all"] : []), ...p.targets.ou_ids, ...(!p.targets.all_endpoints && !p.targets.ou_ids.length ? ["__none"] : [])],
        format: (v) => (v === "__all" ? "All endpoints" : v === "__none" ? "Not linked" : ouPath(ouList, v)) },
      { kind: "enum", key: "setting", label: "Setting", get: (p) => p.settings.map((x) => x.kind), format: (k) => k.replace(/_/g, " ").replace(/^./, (c) => c.toUpperCase()) },
      { kind: "date", key: "modified", label: "Modified", get: (p) => p.updated_at },
    ],
    sorts: {
      priority: { label: "Priority", get: (p) => p.priority, type: "number" },
      name: { label: "Name", get: (p) => p.name },
      link: { label: "Linked to", get: (p) => (p.targets.all_endpoints ? " All endpoints" : p.targets.ou_ids.map((id) => ouPath(ouList, id)).sort()[0] ?? null) },
      settings: { label: "Settings", get: (p) => p.settings.length, type: "number" },
      status: { label: "Status", get: (p) => (p.enabled ? "Enabled" : "Disabled") },
      modified: { label: "Modified", get: (p) => p.updated_at, type: "date" },
    },
    defaultSort: { key: "priority", dir: "asc" },
  });
  return (
    <>
      <PageHeader title="Group Policy" guide="03-group-policy" term="gpo" subtitle="Group Policy Objects that configure Windows, macOS and Linux endpoints from one place."
        actions={<Button variant="primary" icon={<Plus className="size-4" />} onClick={() => nav("/gpos/new")}>New GPO</Button>} />
      <Card padded={false}>
        <ListToolbar view={view} placeholder="Search GPOs and settings" noun={["GPO", "GPOs"]} />
        {ps.loading && !ps.data ? <Loading /> : (
          <Table view={view} head={[{ label: "Priority", sort: "priority" }, { label: "Name", sort: "name" }, { label: "Linked to", sort: "link" }, { label: "Settings", sort: "settings" }, { label: "Status", sort: "status" }, { label: "Modified", sort: "modified" }]}
            empty={view.rows.length === 0 && (view.total > 0 ? <NoMatches view={view} /> : <EmptyState icon={<ShieldCheck className="size-5" />} title="No GPOs yet" body="Create a GPO to set screen lock, firewall, sign-in accounts and more." action={<Button variant="primary" onClick={() => nav("/gpos/new")}>New GPO</Button>} />)}>
            {view.rows.map((p) => (
              <Row key={p.id} onClick={() => nav(`/gpos/${p.id}`)}>
                <Td><span className="inline-grid min-w-9 place-items-center rounded-md bg-panel-2 px-1.5 py-0.5 font-mono text-xs">{p.priority}</span></Td>
                <Td><div className="font-medium">{p.name}</div><div className="max-w-md truncate text-xs text-ink-3">{p.description}</div></Td>
                <Td>
                  <div className="flex flex-wrap gap-1">
                    {p.targets.all_endpoints && <Badge tone="accent"><Globe className="size-3" />All endpoints</Badge>}
                    {p.targets.ou_ids.map((id) => <Badge key={id}>{ouPath(ous.data ?? [], id)}</Badge>)}
                    {!p.targets.all_endpoints && p.targets.ou_ids.length === 0 && <span className="text-xs text-warn">Not linked</span>}
                  </div>
                </Td>
                <Td className="text-ink-2">{p.settings.length}</Td>
                <Td>{p.enabled ? <Badge tone="good">Enabled</Badge> : <Badge>Disabled</Badge>}</Td>
                <Td className="text-ink-3">{ago(p.updated_at)}</Td>
              </Row>
            ))}
          </Table>
        )}
      </Card>
      <div className="mt-5 grid gap-4 md:grid-cols-2">
        <Explain term="priority" />
        <Explain>Want to know what an endpoint actually receives? Open it in <Link to="/endpoints" className="text-accent hover:underline">Endpoints</Link> and check <b>Policy &amp; compliance</b> for its Resultant Set of Policy.</Explain>
      </div>
    </>
  );
}
