// Choose where something runs: all endpoints, or any mix of endpoints,
// security groups (their computer members) and OUs (with child OUs).
import { useState } from "react";
import { FolderTree, Globe, Monitor, UsersRound, X } from "lucide-react";
import type { Endpoint, Group, OrgUnit, RunTargets } from "../lib/types";
import { ouPath, OuSelect } from "../pages/Users";
import { Button, Select, Switch } from "./ui";

export const NO_TARGETS: RunTargets = { all_endpoints: false, endpoint_ids: [], group_ids: [], ou_ids: [] };

export function TargetPicker({ value, onChange, endpoints, groups, ous }: {
  value: RunTargets; onChange: (t: RunTargets) => void; endpoints: Endpoint[]; groups: Group[]; ous: OrgUnit[];
}) {
  const [ep, setEp] = useState("");
  const [gr, setGr] = useState("");
  const [ou, setOu] = useState<string | null>(null);
  const chip = (key: string, icon: React.ReactNode, label: string, remove: () => void) => (
    <span key={key} className="inline-flex items-center gap-1.5 rounded-full bg-panel-2 px-2.5 py-1 text-xs">{icon}{label}
      <button onClick={remove} className="text-ink-3 hover:text-bad" aria-label={`Remove ${label}`}><X className="size-3" /></button></span>
  );
  const add = <K extends "endpoint_ids" | "group_ids" | "ou_ids">(k: K, id: string) => { if (id && !value[k].includes(id)) onChange({ ...value, [k]: [...value[k], id] }); };
  const drop = <K extends "endpoint_ids" | "group_ids" | "ou_ids">(k: K, id: string) => onChange({ ...value, [k]: value[k].filter((x) => x !== id) });
  return (
    <div className="space-y-3">
      <Switch checked={value.all_endpoints} onChange={(v) => onChange({ ...value, all_endpoints: v })} label={<span className="flex items-center gap-1.5"><Globe className="size-3.5" />Every endpoint in the domain</span>} />
      {!value.all_endpoints && (
        <>
          <div className="flex min-h-8 flex-wrap gap-1.5">
            {value.endpoint_ids.map((id) => chip(id, <Monitor className="size-3" />, endpoints.find((e) => e.id === id)?.hostname ?? "Unknown", () => drop("endpoint_ids", id)))}
            {value.group_ids.map((id) => chip(id, <UsersRound className="size-3" />, groups.find((g) => g.id === id)?.name ?? "Unknown", () => drop("group_ids", id)))}
            {value.ou_ids.map((id) => chip(id, <FolderTree className="size-3" />, ouPath(ous, id), () => drop("ou_ids", id)))}
            {value.endpoint_ids.length + value.group_ids.length + value.ou_ids.length === 0 && <span className="text-xs text-ink-3">Nothing chosen yet.</span>}
          </div>
          <div className="grid gap-2 sm:grid-cols-3">
            <div className="flex gap-1.5">
              <Select value={ep} onChange={(e) => setEp(e.target.value)}><option value="">Endpoint…</option>{endpoints.filter((e) => !value.endpoint_ids.includes(e.id)).map((e) => <option key={e.id} value={e.id}>{e.hostname}</option>)}</Select>
              <Button disabled={!ep} onClick={() => { add("endpoint_ids", ep); setEp(""); }}>Add</Button>
            </div>
            <div className="flex gap-1.5">
              <Select value={gr} onChange={(e) => setGr(e.target.value)}><option value="">Security group…</option>{groups.filter((g) => !value.group_ids.includes(g.id)).map((g) => <option key={g.id} value={g.id}>{g.name} ({g.endpoint_ids.length} computers)</option>)}</Select>
              <Button disabled={!gr} onClick={() => { add("group_ids", gr); setGr(""); }}>Add</Button>
            </div>
            <div className="flex gap-1.5">
              <OuSelect ous={ous.filter((o) => !value.ou_ids.includes(o.id))} value={ou} onChange={setOu} none="OU…" />
              <Button disabled={!ou} onClick={() => { add("ou_ids", ou!); setOu(null); }}>Add</Button>
            </div>
          </div>
          <p className="text-xs text-ink-3">Groups target their computer members. OUs include their child OUs, like GPO links.</p>
        </>
      )}
    </div>
  );
}
