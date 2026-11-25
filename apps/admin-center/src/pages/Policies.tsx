import { Link, useNavigate } from "react-router-dom";
import { Globe, Plus, ShieldCheck } from "lucide-react";
import { useQuery } from "../lib/hooks";
import type { OrgUnit, Policy } from "../lib/types";
import { ago } from "../lib/format";
import { Badge, Button, Card, EmptyState, Explain, Loading, PageHeader, Row, Table, Td } from "../components/ui";
import { ouPath } from "./Users";

export default function Policies() {
  const ps = useQuery<Policy[]>("/policies");
  const ous = useQuery<OrgUnit[]>("/ous");
  const nav = useNavigate();
  return (
    <>
      <PageHeader title="Group Policy" term="gpo" subtitle="Group Policy Objects that configure Windows, macOS and Linux endpoints from one place."
        actions={<Button variant="primary" icon={<Plus className="size-4" />} onClick={() => nav("/gpos/new")}>New GPO</Button>} />
      <Card padded={false}>
        {ps.loading && !ps.data ? <Loading /> : (
          <Table head={["Priority", "Name", "Linked to", "Settings", "Status", "Modified"]}
            empty={(ps.data ?? []).length === 0 && <EmptyState icon={<ShieldCheck className="size-5" />} title="No GPOs yet" body="Create a GPO to set screen lock, firewall, sign-in accounts and more." action={<Button variant="primary" onClick={() => nav("/gpos/new")}>New GPO</Button>} />}>
            {(ps.data ?? []).map((p) => (
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
