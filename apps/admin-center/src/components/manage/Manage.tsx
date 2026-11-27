// The endpoint's Manage tab: Services, Apps, Processes and Containers, like
// Computer Management / Windows Admin Center for one machine.
import { useState } from "react";
import { AppWindow, Box, Cog, Gauge } from "lucide-react";
import type { Endpoint } from "../../lib/types";
import { Callout, cx } from "../ui";
import { ServicesPanel } from "./Services";
import { SoftwarePanel } from "./Software";
import { ProcessesPanel } from "./Processes";
import { ContainersPanel } from "./Containers";

type Tool = "services" | "software" | "processes" | "containers";
const TOOLS: { id: Tool; label: string; icon: typeof Cog }[] = [
  { id: "services", label: "Services", icon: Cog },
  { id: "software", label: "Apps", icon: AppWindow },
  { id: "processes", label: "Processes", icon: Gauge },
  { id: "containers", label: "Containers", icon: Box },
];

function newer(v: string, min: string) {
  const p = (x: string) => x.replace(/^v/, "").split(/[.+-]/).map((n) => Number(n) || 0);
  const [a, b] = [p(v), p(min)];
  for (let i = 0; i < 3; i++) if ((a[i] ?? 0) !== (b[i] ?? 0)) return (a[i] ?? 0) > (b[i] ?? 0);
  return true;
}

export function ManageTab({ endpoint }: { endpoint: Endpoint }) {
  const [tool, setTool] = useState<Tool>(() => (sessionStorage.getItem("od.manage.tool") as Tool) || "services");
  const pick = (t: Tool) => { setTool(t); sessionStorage.setItem("od.manage.tool", t); };
  if (!newer(endpoint.agent_version, "0.14.0")) {
    return <Callout tone="warn" title="Update the Endpoint Agent first">{endpoint.hostname} runs agent {endpoint.agent_version || "(unknown)"}. Managing services, apps, processes and containers needs agent 0.14.0 or later. Agents update themselves when agent updates are on.</Callout>;
  }
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap gap-1.5">
        {TOOLS.map((t) => (
          <button key={t.id} type="button" onClick={() => pick(t.id)}
            className={cx("inline-flex h-8 items-center gap-1.5 rounded-md border px-3 text-sm font-medium transition", tool === t.id ? "border-accent bg-accent-soft text-ink" : "border-line bg-panel text-ink-2 hover:bg-panel-2")}>
            <t.icon className="size-4" />{t.label}
          </button>
        ))}
        {!endpoint.online && <span className="ml-auto self-center text-xs text-warn">{endpoint.hostname} looks offline, so live views may time out.</span>}
      </div>
      <div className="overflow-hidden rounded-xl border border-line bg-panel">
        {tool === "services" && <ServicesPanel endpoint={endpoint} />}
        {tool === "software" && <SoftwarePanel endpoint={endpoint} />}
        {tool === "processes" && <ProcessesPanel endpoint={endpoint} />}
        {tool === "containers" && <ContainersPanel endpoint={endpoint} />}
      </div>
      <p className="text-xs text-ink-3">Lists are read live from the agent. Changes run as jobs, show in the job history and are recorded in the audit log. A GPO's Management tools setting can turn each tool off.</p>
    </div>
  );
}
