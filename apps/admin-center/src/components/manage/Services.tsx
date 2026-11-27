// Services: Windows SCM, systemd units or launchd jobs, like services.msc.
import { useState } from "react";
import { Play, RotateCw, Square } from "lucide-react";
import type { Endpoint, ServiceInfo, ServiceReport, StartupType } from "../../lib/types";
import { ListToolbar, NoMatches, useListView } from "../DataView";
import { Button, Confirm, Row, Select, Spinner, Table, Td } from "../ui";
import { LastOutput, ManageError, RefreshButton, StateBadge, useActions, useManage } from "./common";

const STARTUP_LABEL: Record<string, string> = {
  automatic: "Automatic", automatic_delayed: "Automatic (Delayed Start)", manual: "Manual", disabled: "Disabled",
  static: "Static", boot: "Boot", system: "System", generated: "Generated", indirect: "Indirect", alias: "Alias", transient: "Transient",
};
const MANAGER_LABEL = { scm: "Service Control Manager", systemd: "systemd", launchd: "launchd" } as const;

export function ServicesPanel({ endpoint }: { endpoint: Endpoint }) {
  const q = useManage<ServiceReport>(endpoint, "services");
  const act = useActions(endpoint, () => void q.reload());
  const [confirmStop, setConfirmStop] = useState<ServiceInfo | null>(null);
  const mgr = q.data?.manager;
  const view = useListView(q.data?.services, {
    id: `services.${endpoint.id}`,
    search: (s) => [s.name, s.display_name, s.description, s.account],
    filters: [
      { kind: "enum", key: "state", label: "Status", get: (s) => s.state },
      { kind: "enum", key: "startup", label: "Startup type", get: (s) => s.startup, format: (v) => STARTUP_LABEL[v] ?? v },
      { kind: "toggle", key: "running", label: "Running", test: (s) => s.state === "running" },
    ],
    sorts: {
      name: { label: "Name", get: (s) => s.display_name || s.name },
      state: { label: "Status", get: (s) => s.state },
      startup: { label: "Startup type", get: (s) => STARTUP_LABEL[s.startup] ?? s.startup },
      account: { label: "Log on as", get: (s) => s.account },
    },
    defaultSort: { key: "name", dir: "asc" },
  });

  if (q.error && !q.data) return <ManageError error={q.error} onRetry={q.reload} />;
  const options: StartupType[] = mgr === "scm" ? ["automatic", "automatic_delayed", "manual", "disabled"] : mgr === "launchd" ? ["automatic", "disabled"] : ["automatic", "manual", "disabled"];
  const control = (s: ServiceInfo, c: "start" | "stop" | "restart") =>
    act.run(`${s.name}:${c}`, { op: "service", name: s.name, control: c }, `${s.display_name || s.name}: ${c === "stop" ? "stopped" : c === "start" ? "started" : "restarted"}`);

  return (
    <>
      <ListToolbar view={view} placeholder="Search services" noun={["service", "services"]}
        extra={<RefreshButton loading={q.loading} onRefresh={q.reload} note={mgr && <span>{MANAGER_LABEL[mgr]}</span>} />} />
      {!q.data ? <div className="grid place-items-center p-10"><Spinner /></div> : view.rows.length === 0 ? <NoMatches view={view} /> : (
        <Table view={view} head={[{ label: "Name", sort: "name" }, { label: "Status", sort: "state" }, { label: "Startup type", sort: "startup" }, { label: "Log on as", sort: "account" }, ""]}>
          {view.rows.map((s) => {
            const busy = Object.keys(act.busy).some((k) => k.startsWith(`${s.name}:`));
            const running = s.state === "running";
            return (
              <Row key={s.name}>
                <Td className="max-w-[420px]">
                  <div className="truncate font-medium" title={s.description || undefined}>{s.display_name || s.name}</div>
                  {s.display_name && s.display_name !== s.name && <div className="truncate font-mono text-[11.5px] text-ink-3">{s.name}</div>}
                </Td>
                <Td><StateBadge state={s.state} />{s.pid ? <span className="ml-2 font-mono text-[11px] text-ink-3">PID {s.pid}</span> : null}</Td>
                <Td>
                  {s.configurable ? (
                    <Select className="h-7 w-52 text-[13px]" value={s.startup} disabled={busy}
                      onChange={(e) => void act.run(`${s.name}:startup`, { op: "service_startup", name: s.name, startup: e.target.value as StartupType }, `${s.display_name || s.name}: startup type changed`)}>
                      {!options.includes(s.startup as StartupType) && <option value={s.startup}>{STARTUP_LABEL[s.startup] ?? s.startup}</option>}
                      {options.map((o) => <option key={o} value={o}>{STARTUP_LABEL[o]}</option>)}
                    </Select>
                  ) : <span className="text-[13px] text-ink-3">{STARTUP_LABEL[s.startup] ?? s.startup}</span>}
                </Td>
                <Td className="text-[13px] text-ink-2">{s.account || "—"}</Td>
                <Td className="whitespace-nowrap text-right">
                  {busy ? <Spinner /> : <>
                    {!running && <Button size="sm" variant="ghost" icon={<Play className="size-3.5" />} onClick={() => void control(s, "start")} disabled={s.startup === "disabled"}>Start</Button>}
                    {running && <Button size="sm" variant="ghost" icon={<Square className="size-3.5" />} onClick={() => setConfirmStop(s)}>Stop</Button>}
                    {running && <Button size="sm" variant="ghost" icon={<RotateCw className="size-3.5" />} onClick={() => void control(s, "restart")}>Restart</Button>}
                  </>}
                </Td>
              </Row>
            );
          })}
        </Table>
      )}
      <LastOutput job={act.last} onClose={act.clearLast} />
      <Confirm open={!!confirmStop} onClose={() => setConfirmStop(null)} title={`Stop ${confirmStop?.display_name || confirmStop?.name}?`} confirmLabel="Stop service" danger
        body="Anything that depends on this service stops working until it's started again. Every action is recorded in the audit log."
        onConfirm={async () => { if (confirmStop) void control(confirmStop, "stop"); }} />
    </>
  );
}
