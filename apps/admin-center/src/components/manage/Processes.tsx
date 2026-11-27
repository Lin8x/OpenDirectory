// Processes: a Task Manager view with End task and End process tree.
import { useState } from "react";
import { Pause, Play, XCircle } from "lucide-react";
import type { Endpoint, ProcessInfo, ProcessReport } from "../../lib/types";
import { bytes, uptime } from "../../lib/format";
import { ListToolbar, NoMatches, useListView } from "../DataView";
import { Button, Confirm, Row, Spinner, Switch, Table, Td } from "../ui";
import { ManageError, RefreshButton, useActions, useManage } from "./common";

const LIMIT = 300;

export function ProcessesPanel({ endpoint }: { endpoint: Endpoint }) {
  const [live, setLive] = useState(true);
  const q = useManage<ProcessReport>(endpoint, "processes", live ? 5000 : undefined);
  const act = useActions(endpoint, () => void q.reload());
  const [kill, setKill] = useState<ProcessInfo | null>(null);
  const [tree, setTree] = useState(false);
  const view = useListView(q.data?.processes, {
    id: `processes.${endpoint.id}`,
    search: (p) => [p.name, p.pid, p.user, p.command],
    filters: [{ kind: "enum", key: "user", label: "User", get: (p) => p.user || null }],
    sorts: {
      name: { label: "Name", get: (p) => p.name },
      pid: { label: "PID", get: (p) => p.pid, type: "number" },
      user: { label: "User", get: (p) => p.user },
      cpu: { label: "CPU", get: (p) => p.cpu_percent, type: "number" },
      mem: { label: "Memory", get: (p) => p.memory_bytes, type: "number" },
      time: { label: "Running for", get: (p) => p.run_secs, type: "number" },
    },
    defaultSort: { key: "cpu", dir: "desc" },
  });
  if (q.error && !q.data) return <ManageError error={q.error} onRetry={q.reload} />;
  const d = q.data;

  return (
    <>
      {d && (
        <div className="grid grid-cols-2 gap-px border-b border-line bg-line sm:grid-cols-3">
          <Meter label="CPU" value={`${d.cpu_percent.toFixed(0)}%`} pct={d.cpu_percent / 100} sub={`${d.cpu_count} logical processors`} />
          <Meter label="Memory" value={bytes(d.memory_used)} pct={d.memory_total ? d.memory_used / d.memory_total : 0} sub={`of ${bytes(d.memory_total)}`} />
          <div className="hidden bg-panel px-4 py-3 sm:block"><div className="text-xs text-ink-3">Processes</div><div className="text-lg font-semibold">{d.processes.length}</div></div>
        </div>
      )}
      <ListToolbar view={view} placeholder="Search name, PID, user or command line" noun={["process", "processes"]}
        extra={<>
          <Button size="sm" variant="ghost" icon={live ? <Pause className="size-3.5" /> : <Play className="size-3.5" />} onClick={() => setLive(!live)}>{live ? "Pause updates" : "Resume updates"}</Button>
          <RefreshButton loading={q.loading} onRefresh={q.reload} />
        </>} />
      {!d ? <div className="grid place-items-center p-10"><Spinner /></div> : view.rows.length === 0 ? <NoMatches view={view} /> : (
        <>
          <Table view={view} head={[{ label: "Name", sort: "name" }, { label: "PID", sort: "pid" }, { label: "User", sort: "user" }, { label: "CPU", sort: "cpu" }, { label: "Memory", sort: "mem" }, { label: "Running for", sort: "time" }, ""]}>
            {view.rows.slice(0, LIMIT).map((p) => (
              <Row key={p.pid}>
                <Td className="max-w-[360px]">
                  <div className="truncate font-medium">{p.name}</div>
                  {p.command && <div className="truncate font-mono text-[11px] text-ink-3" title={p.command}>{p.command}</div>}
                </Td>
                <Td className="font-mono text-[12px]">{p.pid}</Td>
                <Td className="text-[13px] text-ink-2">{p.user || "—"}</Td>
                <Td className="font-mono text-[12px]">{p.cpu_percent.toFixed(1)}%</Td>
                <Td className="font-mono text-[12px]">{bytes(p.memory_bytes)}</Td>
                <Td className="text-[12px] text-ink-3">{p.run_secs ? uptime(p.run_secs) : "—"}</Td>
                <Td className="text-right">
                  {act.busy[`kill:${p.pid}`] ? <Spinner /> : <Button size="sm" variant="ghost" icon={<XCircle className="size-3.5" />} onClick={() => { setTree(false); setKill(p); }}>End task</Button>}
                </Td>
              </Row>
            ))}
          </Table>
          {view.rows.length > LIMIT && <p className="border-t border-line px-4 py-2.5 text-xs text-ink-3">Showing {LIMIT} of {view.rows.length}. Search to narrow the list.</p>}
        </>
      )}
      <Confirm open={!!kill} onClose={() => setKill(null)} danger title={`End ${kill?.name} (PID ${kill?.pid})?`} confirmLabel={tree ? "End process tree" : "End task"}
        body={<div className="space-y-3">
          <p>Unsaved work in this program is lost. The agent asks it to exit, then forces it after 3 seconds.</p>
          <Switch checked={tree} onChange={setTree} label="Also end the processes it started (End process tree)" />
        </div>}
        onConfirm={async () => { if (kill) void act.run(`kill:${kill.pid}`, { op: "kill_process", pid: kill.pid, tree, expect_name: kill.name }); }} />
    </>
  );
}

function Meter({ label, value, pct, sub }: { label: string; value: string; pct: number; sub: string }) {
  return (
    <div className="bg-panel px-4 py-3">
      <div className="flex items-baseline justify-between"><span className="text-xs text-ink-3">{label}</span><span className="text-lg font-semibold">{value}</span></div>
      <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-panel-2"><div className={`h-full rounded-full ${pct > 0.9 ? "bg-bad" : pct > 0.75 ? "bg-warn" : "bg-accent"}`} style={{ width: `${Math.min(100, pct * 100)}%` }} /></div>
      <div className="mt-1 text-[11px] text-ink-3">{sub}</div>
    </div>
  );
}
