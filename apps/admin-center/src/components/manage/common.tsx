// Shared pieces for endpoint management: live queries to the agent, actions
// that run as jobs (we wait for the result), and the "busy row" state.
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { RefreshCw } from "lucide-react";
import { get, post } from "../../lib/api";
import type { Endpoint, Job, ManageAction } from "../../lib/types";
import { Button, Callout, Spinner, useToast } from "../ui";

/** Ask the endpoint's agent for a live report. Not cached on the server. */
export function useManage<T>(endpoint: Endpoint, area: "services" | "software" | "processes" | "containers", poll?: number) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const seq = useRef(0);
  const inFlight = useRef(false);
  const load = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    const n = ++seq.current;
    setLoading(true);
    try {
      const d = await get<T>(`/endpoints/${endpoint.id}/manage/${area}`);
      if (n === seq.current) { setData(d); setError(null); }
    } catch (e) {
      if (n === seq.current) setError((e as Error).message);
    } finally {
      inFlight.current = false;
      if (n === seq.current) setLoading(false);
    }
  }, [endpoint.id, area]);
  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    if (!poll) return;
    const t = setInterval(() => { if (document.visibilityState === "visible") void load(); }, poll);
    return () => clearInterval(t);
  }, [poll, load]);
  return { data, error, loading, reload: load };
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Queue an action and wait until the agent reports back (up to 30 minutes). */
export async function runAction(endpointId: string, action: ManageAction): Promise<Job> {
  let job = await post<Job>(`/endpoints/${endpointId}/manage/actions`, action);
  const until = Date.now() + 30 * 60_000;
  let wait = 700;
  while ((job.status === "queued" || job.status === "running") && Date.now() < until) {
    await sleep(wait);
    wait = Math.min(wait * 1.3, 4000);
    job = await get<Job>(`/jobs/${job.id}`);
  }
  return job;
}

/** Run an action with a busy marker and a toast for the outcome. */
export function useActions(endpoint: Endpoint, after?: () => void) {
  const toast = useToast();
  const [busy, setBusy] = useState<Record<string, boolean>>({});
  const [last, setLast] = useState<Job | null>(null);
  const run = async (key: string, action: ManageAction, done?: string) => {
    setBusy((b) => ({ ...b, [key]: true }));
    try {
      const j = await runAction(endpoint.id, action);
      setLast(j);
      if (j.status === "succeeded") toast("good", done ?? firstLine(j.output));
      else if (j.status === "queued") toast("info", `${endpoint.hostname} hasn't picked this up yet. It runs when the endpoint is back online.`);
      else toast("bad", firstLine(j.output) || "The action failed");
      after?.();
      return j;
    } catch (e) {
      toast("bad", (e as Error).message);
      return null;
    } finally {
      setBusy((b) => { const n = { ...b }; delete n[key]; return n; });
    }
  };
  return { busy, run, last, clearLast: () => setLast(null) };
}

export function firstLine(s: string) {
  return s.split("\n").map((l) => l.trim()).filter(Boolean).slice(-1)[0] ?? "";
}

/** Refresh button and a note, for the right side of a list toolbar. */
export function RefreshButton({ loading, onRefresh, note }: { loading: boolean; onRefresh: () => void; note?: ReactNode }) {
  return (
    <span className="flex items-center gap-2 text-xs text-ink-3">
      {note}
      {loading && <Spinner />}
      <Button size="sm" variant="ghost" icon={<RefreshCw className="size-3.5" />} onClick={onRefresh}>Refresh</Button>
    </span>
  );
}

/** Error from the agent or a permission/GPO refusal, with retry. */
export function ManageError({ error, onRetry }: { error: string; onRetry: () => void }) {
  return (
    <div className="p-4">
      <Callout tone="warn" title="Couldn't load this from the endpoint">
        <p>{error}</p>
        <Button size="sm" className="mt-2" onClick={onRetry}>Try again</Button>
      </Callout>
    </div>
  );
}

/** The output of the last action, for things like package installs. */
export function LastOutput({ job, onClose }: { job: Job | null; onClose: () => void }) {
  if (!job || !job.output.trim()) return null;
  return (
    <div className="border-t border-line bg-panel-2/50 p-3">
      <div className="mb-1 flex items-center justify-between text-xs text-ink-3">
        <span>Output of the last action ({job.status})</span>
        <button onClick={onClose} className="hover:text-ink">Hide</button>
      </div>
      <pre className="selectable max-h-56 overflow-auto whitespace-pre-wrap rounded-md bg-[#0f1218] p-2.5 font-mono text-[11.5px] text-[#d6dbe4]">{job.output}</pre>
    </div>
  );
}

export function StateBadge({ state }: { state: string }) {
  const tone = ["running", "active"].includes(state) ? "text-good" : ["failed", "dead", "exited"].includes(state) ? "text-bad" : ["starting", "stopping", "pending", "restarting", "paused"].includes(state) ? "text-warn" : "text-ink-3";
  const dot = ["running", "active"].includes(state) ? "bg-good" : ["failed", "dead"].includes(state) ? "bg-bad" : ["starting", "stopping", "pending", "restarting", "paused"].includes(state) ? "bg-warn" : "bg-ink-3/50";
  return <span className={`inline-flex items-center gap-1.5 text-[13px] capitalize ${tone}`}><span className={`size-1.5 rounded-full ${dot}`} />{state || "unknown"}</span>;
}
