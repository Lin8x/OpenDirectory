import { useState, type ReactNode } from "react";
import { Link } from "react-router-dom";
import { ChevronDown, ChevronRight, Terminal, X } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { post } from "../lib/api";
import type { Endpoint, Job, OsFamily } from "../lib/types";
import { ago, dateTime, osLabel } from "../lib/format";
import { ListToolbar, NoMatches, useListView } from "../components/DataView";
import { Badge, Button, Card, CopyButton, EmptyState, Explain, Loading, OsIcon, PageHeader, Select, useToast } from "../components/ui";

const tone = { queued: "neutral", running: "accent", succeeded: "good", failed: "bad", cancelled: "neutral" } as const;

export function jobTitle(j: Job) {
  if (j.kind.type === "script") return j.kind.script.split("\n")[0]!.slice(0, 80) || "Script";
  if (j.kind.type === "scan_updates") return "Scan for updates";
  if (j.kind.type === "install_updates") return j.kind.security_only ? "Install security updates" : "Install updates";
  return j.kind.type === "refresh_inventory" ? "Refresh inventory" : "Restart agent";
}

export function JobRow({ job, endpoint, onChanged, defaultOpen = false }: { job: Job; endpoint?: Endpoint; onChanged?: () => void; defaultOpen?: boolean }) {
  const [open, setOpen] = useState(defaultOpen);
  const toast = useToast();
  return (
    <li>
      <button onClick={() => setOpen(!open)} className="flex w-full items-center gap-3 px-4 py-2.5 text-left hover:bg-panel-2">
        {open ? <ChevronDown className="size-4 text-ink-3" /> : <ChevronRight className="size-4 text-ink-3" />}
        <div className="min-w-0 flex-1">
          <div className="truncate font-mono text-[12.5px]">{jobTitle(job)}</div>
          <div className="text-xs text-ink-3">
            {endpoint && <><Link to={`/endpoints/${endpoint.id}`} onClick={(e) => e.stopPropagation()} className="text-accent hover:underline">{endpoint.hostname}</Link> · </>}
            {job.kind.type === "script" && <>{job.kind.shell} · </>}{job.created_by} · {ago(job.created_at)}
          </div>
        </div>
        {job.exit_code !== null && <span className="font-mono text-xs text-ink-3">exit {job.exit_code}</span>}
        <Badge tone={tone[job.status]}>{job.status[0]!.toUpperCase() + job.status.slice(1)}</Badge>
      </button>
      {open && (
        <div className="space-y-2 px-4 pb-3 pl-11">
          {job.kind.type === "script" && <pre className="selectable max-h-40 overflow-auto rounded-md bg-panel-2 px-3 py-2 font-mono text-xs text-ink-2">{job.kind.script}</pre>}
          <div className="flex items-center justify-between text-xs font-medium text-ink-3"><span>Output</span>{job.output && <CopyButton text={job.output} />}</div>
          <pre className="selectable max-h-80 overflow-auto rounded-md bg-[#0f1218] px-3 py-2 font-mono text-xs leading-relaxed text-[#d7dce5]">{job.output || (job.status === "queued" ? "Waiting for the endpoint to check in…" : job.status === "running" ? "Running…" : "(no output)")}</pre>
          <div className="flex items-center gap-3 text-xs text-ink-3">
            <span>Queued {dateTime(job.created_at)}</span>{job.finished_at && <span>Finished {dateTime(job.finished_at)}</span>}
            {job.status === "queued" && <Button size="sm" variant="ghost" icon={<X className="size-3.5" />} onClick={async () => { try { await post(`/jobs/${job.id}/cancel`); onChanged?.(); } catch (e) { toast("bad", (e as Error).message); } }}>Cancel</Button>}
          </div>
        </div>
      )}
    </li>
  );
}

const STATUS_LABEL: Record<Job["status"], string> = { queued: "Queued", running: "Running", succeeded: "Succeeded", failed: "Failed", cancelled: "Cancelled" };
const KIND_LABEL: Record<Job["kind"]["type"], string> = {
  script: "Script", scan_updates: "Update scan", install_updates: "Update install", refresh_inventory: "Inventory refresh", restart_agent: "Agent restart", notify_user: "User notification",
};

/** Filterable, sortable list of script and job runs. Shared by Remote Scripts and the Script Library. */
export function JobList({ path, id, empty }: { path: string; id: string; empty: ReactNode }) {
  const jobs = useQuery<Job[]>(path, { poll: 4000 });
  const eps = useQuery<Endpoint[]>("/endpoints");
  const byId = new Map((eps.data ?? []).map((e) => [e.id, e]));
  const host = (j: Job) => byId.get(j.endpoint_id)?.hostname ?? null;
  const view = useListView(jobs.data, {
    id,
    search: (j) => [jobTitle(j), host(j), j.created_by, j.kind.type === "script" ? j.kind.script : null, j.output.slice(0, 4000)],
    filters: [
      { kind: "enum", key: "status", label: "Status", get: (j) => j.status, options: (Object.keys(STATUS_LABEL) as Job["status"][]).map((k) => ({ value: k, label: STATUS_LABEL[k] })) },
      { kind: "enum", key: "kind", label: "Type", get: (j) => j.kind.type, format: (k) => KIND_LABEL[k as Job["kind"]["type"]] ?? k },
      { kind: "enum", key: "endpoint", label: "Endpoint", get: (j) => host(j) },
      { kind: "enum", key: "os", label: "OS", get: (j) => byId.get(j.endpoint_id)?.os ?? null, format: (o) => osLabel[o as OsFamily] ?? o },
      { kind: "enum", key: "by", label: "Run by", get: (j) => j.created_by },
      { kind: "date", key: "queued", label: "Queued", get: (j) => j.created_at },
    ],
    sorts: {
      queued: { label: "Queued", get: (j) => j.created_at, type: "date" },
      finished: { label: "Finished", get: (j) => j.finished_at, type: "date" },
      endpoint: { label: "Endpoint", get: host },
      status: { label: "Status", get: (j) => STATUS_LABEL[j.status] },
      by: { label: "Run by", get: (j) => j.created_by },
    },
    defaultSort: { key: "queued", dir: "desc" },
  });
  if (jobs.loading && !jobs.data) return <Card><Loading /></Card>;
  if ((jobs.data ?? []).length === 0) return <Card>{empty}</Card>;
  return (
    <Card padded={false}>
      <ListToolbar view={view} placeholder="Search script, output, endpoint or user" sortPicker noun={["run", "runs"]} />
      {view.rows.length === 0 ? <NoMatches view={view} /> : (
        <ul className="divide-y divide-line">
          {view.rows.map((j) => {
            const e = byId.get(j.endpoint_id);
            return <div key={j.id} className="flex">{e && <span className="grid w-10 place-items-center pl-3"><OsIcon os={e.os} /></span>}<div className="flex-1"><JobRow job={j} endpoint={e} onChanged={jobs.reload} /></div></div>;
          })}
        </ul>
      )}
    </Card>
  );
}

export default function Jobs() {
  const eps = useQuery<Endpoint[]>("/endpoints");
  const [target, setTarget] = useState("");
  return (
    <>
      <PageHeader title="Remote Scripts" subtitle="Run PowerShell, bash, zsh or cmd on any endpoint and see the output here. Every run is recorded in the audit log." term="rmm"
        actions={<div className="flex items-center gap-2">
          <Select value={target} onChange={(e) => setTarget(e.target.value)} className="w-56">
            <option value="">Choose an endpoint…</option>
            {(eps.data ?? []).map((e) => <option key={e.id} value={e.id}>{e.hostname}</option>)}
          </Select>
          <Link to={target ? `/endpoints/${target}` : "#"}><Button variant="primary" disabled={!target} icon={<Terminal className="size-4" />}>New script</Button></Link>
        </div>} />
      <JobList path="/jobs?limit=200" id="jobs" empty={<EmptyState icon={<Terminal className="size-5" />} title="No scripts yet" body="Open an endpoint and use the Remote scripts tab, or pick one above." />} />
      <div className="mt-5"><Explain>Scripts run with full system rights (SYSTEM on Windows, root on macOS and Linux) and time out after 30 minutes. Output is capped at 1 MB.</Explain></div>
    </>
  );
}
