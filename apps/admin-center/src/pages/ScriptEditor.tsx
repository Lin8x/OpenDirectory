// Edit a library script, run it against endpoints, groups or OUs, and see
// where it's used and how it went.
import { useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { ArrowLeft, Play, Plus, Save, ShieldCheck, Trash2, X } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { del, post, put } from "../lib/api";
import type { Endpoint, Group, OrgUnit, OsFamily, RunResponse, RunTargets, Script, ScriptParam, Shell } from "../lib/types";
import { Badge, Button, Callout, Card, Confirm, Dialog, Field, Input, Loading, OsIcon, Select, Textarea, cx, useToast } from "../components/ui";
import { NO_TARGETS, TargetPicker } from "../components/TargetPicker";
import { RunHistory } from "./Scripts";

type Draft = Pick<Script, "name" | "description" | "category" | "shell" | "body" | "os" | "params">;
const EMPTY: Draft = { name: "", description: "", category: "General", shell: "default", body: "", os: [], params: [] };
const OSES: OsFamily[] = ["windows", "macos", "linux"];
const SHELLS: { v: Shell; l: string }[] = [
  { v: "default", l: "Default (cmd on Windows, sh elsewhere)" }, { v: "powershell", l: "PowerShell" }, { v: "bash", l: "bash" }, { v: "zsh", l: "zsh" }, { v: "cmd", l: "cmd" },
];

export default function ScriptEditor() {
  const { id } = useParams();
  const isNew = id === "new";
  const nav = useNavigate();
  const toast = useToast();
  const existing = useQuery<Script>(isNew ? null : `/scripts/${id}`);
  const usage = useQuery<{ gpos: string[] }>(isNew ? null : `/scripts/${id}/usage`);
  const [d, setD] = useState<Draft | null>(isNew ? EMPTY : null);
  const [saving, setSaving] = useState(false);
  const [running, setRunning] = useState(false);
  const [confirm, setConfirm] = useState(false);
  useEffect(() => { if (existing.data) setD(pick(existing.data)); }, [existing.data]);
  if (!d) return <Loading />;
  const dirty = isNew || !existing.data || JSON.stringify(d) !== JSON.stringify(pick(existing.data));

  const save = async () => {
    setSaving(true);
    try {
      if (isNew) { const s = await post<Script>("/scripts", d); toast("good", "Script saved to the library"); nav(`/scripts/${s.id}`, { replace: true }); }
      else { await put(`/scripts/${id}`, d); toast("good", "Saved. GPOs that use it pick up the change at the next check-in."); void existing.reload(); }
    } catch (e) { toast("bad", (e as Error).message); } finally { setSaving(false); }
  };
  const setParam = (i: number, p: ScriptParam) => setD({ ...d, params: d.params.map((x, j) => (j === i ? p : x)) });

  return (
    <div className="space-y-5">
      <Link to="/scripts" className="inline-flex items-center gap-1 text-sm text-ink-3 hover:text-ink"><ArrowLeft className="size-4" />Script Library</Link>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-[22px] font-semibold tracking-tight">{isNew ? "New script" : d.name || "Untitled script"}</h1>
        <div className="flex gap-2">
          {!isNew && <Button variant="ghost" icon={<Trash2 className="size-4" />} onClick={() => setConfirm(true)}>Delete</Button>}
          {!isNew && <Button icon={<Play className="size-4" />} disabled={dirty} title={dirty ? "Save your changes first" : undefined} onClick={() => setRunning(true)}>Run…</Button>}
          <Button variant="primary" icon={<Save className="size-4" />} loading={saving} disabled={!d.name.trim() || !d.body.trim() || !dirty} onClick={save}>{isNew ? "Save to library" : "Save"}</Button>
        </div>
      </div>

      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_340px]">
        <div className="space-y-5">
          <Card title="Script">
            <div className="space-y-3">
              <Field label="Shell"><Select value={d.shell} onChange={(e) => setD({ ...d, shell: e.target.value as Shell })}>{SHELLS.map((s) => <option key={s.v} value={s.v}>{s.l}</option>)}</Select></Field>
              <Textarea rows={16} spellCheck={false} className="font-mono text-[12.5px] leading-relaxed" value={d.body} onChange={(e) => setD({ ...d, body: e.target.value })}
                placeholder={"# Read parameters from the environment, for example:\n# PowerShell: $env:OD_PARAM_NAME\n# bash/zsh:   \"$OD_PARAM_NAME\""} />
            </div>
          </Card>
          <Card title="Parameters" subtitle="Values you're asked for when it runs. Each becomes an environment variable."
            actions={<Button size="sm" icon={<Plus className="size-3.5" />} onClick={() => setD({ ...d, params: [...d.params, { name: "", label: "", default: "", required: false }] })}>Add parameter</Button>}>
            {d.params.length === 0 ? <p className="text-sm text-ink-3">No parameters. The script always runs the same way.</p> : (
              <div className="space-y-2">
                {d.params.map((p, i) => (
                  <div key={i} className="grid items-end gap-2 sm:grid-cols-[1fr_1.4fr_1fr_auto_auto]">
                    <Field label="Name"><Input className="font-mono text-xs" value={p.name} placeholder="USERNAME" onChange={(e) => setParam(i, { ...p, name: e.target.value.toUpperCase().replace(/[^A-Z0-9_]/g, "_") })} /></Field>
                    <Field label="Label"><Input value={p.label} placeholder="User to unlock" onChange={(e) => setParam(i, { ...p, label: e.target.value })} /></Field>
                    <Field label="Default"><Input value={p.default} onChange={(e) => setParam(i, { ...p, default: e.target.value })} /></Field>
                    <label className="flex h-9 items-center gap-1.5 text-xs"><input type="checkbox" checked={p.required} onChange={(e) => setParam(i, { ...p, required: e.target.checked })} />Required</label>
                    <button className="mb-2 text-ink-3 hover:text-bad" onClick={() => setD({ ...d, params: d.params.filter((_, j) => j !== i) })} aria-label="Remove parameter"><X className="size-4" /></button>
                  </div>
                ))}
                <p className="text-xs text-ink-3">In the script use <code className="font-mono">$env:OD_PARAM_NAME</code> (PowerShell), <code className="font-mono">%OD_PARAM_NAME%</code> (cmd) or <code className="font-mono">"$OD_PARAM_NAME"</code> (bash/zsh).</p>
              </div>
            )}
          </Card>
          {!isNew && <div><h3 className="mb-2 text-sm font-semibold">Run history</h3><RunHistory path={`/scripts/${id}/jobs?limit=100`} /></div>}
        </div>
        <div className="space-y-5">
          <Card title="Details">
            <div className="space-y-3">
              <Field label="Name"><Input value={d.name} onChange={(e) => setD({ ...d, name: e.target.value })} autoFocus={isNew} placeholder="Unlock a user account" /></Field>
              <Field label="Description"><Textarea rows={3} value={d.description} onChange={(e) => setD({ ...d, description: e.target.value })} /></Field>
              <Field label="Category"><Input value={d.category} onChange={(e) => setD({ ...d, category: e.target.value })} placeholder="Maintenance" /></Field>
              <Field label="Runs on" hint="Endpoints on other OSs are skipped. Choose none for any OS.">
                <div className="flex gap-1.5">
                  {OSES.map((o) => {
                    const on = d.os.includes(o);
                    return <button key={o} onClick={() => setD({ ...d, os: on ? d.os.filter((x) => x !== o) : [...d.os, o] })}
                      className={cx("flex items-center gap-1.5 rounded-md border px-2.5 py-1.5 text-xs", on ? "border-accent bg-accent-soft text-accent" : "border-line text-ink-2")}><OsIcon os={o} className="size-3.5" />{o === "macos" ? "macOS" : o[0]!.toUpperCase() + o.slice(1)}</button>;
                  })}
                </div>
              </Field>
            </div>
          </Card>
          {!isNew && (
            <Card title={<span className="flex items-center gap-2"><ShieldCheck className="size-4" />Used by GPOs</span>}>
              {(usage.data?.gpos ?? []).length === 0
                ? <p className="text-sm text-ink-3">Not attached to any GPO. Open a GPO and add a <b>Scripts</b> setting to run this at startup, at sign-in or on a schedule.</p>
                : <ul className="space-y-1 text-sm">{usage.data!.gpos.map((g) => <li key={g}>{g}</li>)}</ul>}
            </Card>
          )}
        </div>
      </div>

      {running && existing.data && <RunDialog script={existing.data} onClose={() => setRunning(false)} />}
      <Confirm open={confirm} onClose={() => setConfirm(false)} danger title={`Delete ${d.name}?`} confirmLabel="Delete script"
        body="Its run history stays. Scripts attached to a GPO must be removed from the GPO first."
        onConfirm={async () => { try { await del(`/scripts/${id}`); toast("good", "Script deleted"); nav("/scripts"); } catch (e) { toast("bad", (e as Error).message); } }} />
    </div>
  );
}

function pick(s: Script): Draft {
  return { name: s.name, description: s.description, category: s.category, shell: s.shell, body: s.body, os: s.os, params: s.params };
}

function RunDialog({ script, onClose }: { script: Script; onClose: () => void }) {
  const toast = useToast();
  const nav = useNavigate();
  const eps = useQuery<Endpoint[]>("/endpoints");
  const groups = useQuery<Group[]>("/groups");
  const ous = useQuery<OrgUnit[]>("/ous");
  const [targets, setTargets] = useState<RunTargets>(NO_TARGETS);
  const [values, setValues] = useState<Record<string, string>>(Object.fromEntries(script.params.map((p) => [p.name, p.default])));
  const [preview, setPreview] = useState<RunResponse | null>(null);
  const [busy, setBusy] = useState(false);
  const empty = !targets.all_endpoints && targets.endpoint_ids.length + targets.group_ids.length + targets.ou_ids.length === 0;

  useEffect(() => {
    if (empty) { setPreview(null); return; }
    const t = setTimeout(() => {
      post<RunResponse>(`/scripts/${script.id}/run`, { targets, params: values, dry_run: true }).then(setPreview).catch(() => setPreview(null));
    }, 250);
    return () => clearTimeout(t);
  }, [targets, empty, script.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const n = preview?.targets.length ?? 0;
  const run = async () => {
    setBusy(true);
    try {
      const r = await post<RunResponse>(`/scripts/${script.id}/run`, { targets, params: values });
      toast("good", `Queued on ${r.jobs.length} endpoint${r.jobs.length === 1 ? "" : "s"}. Results appear as each one checks in.`);
      onClose();
      nav(`/scripts/${script.id}`);
    } catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
  };
  return (
    <Dialog open onClose={onClose} wide title={<span className="flex items-center gap-2"><Play className="size-4" />Run {script.name}</span>}
      footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" loading={busy} disabled={n === 0} icon={<Play className="size-4" />} onClick={run}>{n ? `Run on ${n} endpoint${n === 1 ? "" : "s"}` : "Run"}</Button></>}>
      <div className="space-y-4">
        <Field label="Targets"><TargetPicker value={targets} onChange={setTargets} endpoints={eps.data ?? []} groups={groups.data ?? []} ous={ous.data ?? []} /></Field>
        {script.params.length > 0 && (
          <div className="grid gap-3 sm:grid-cols-2">
            {script.params.map((p) => <Field key={p.name} label={`${p.label || p.name}${p.required ? " *" : ""}`}><Input value={values[p.name] ?? ""} onChange={(e) => setValues({ ...values, [p.name]: e.target.value })} /></Field>)}
          </div>
        )}
        {preview && (
          <div className="rounded-lg border border-line p-3 text-sm">
            <div className="mb-2 font-medium">{n} endpoint{n === 1 ? "" : "s"} will run it{preview.skipped.length > 0 && <span className="font-normal text-ink-3">, {preview.skipped.length} skipped (wrong OS)</span>}</div>
            <div className="flex max-h-28 flex-wrap gap-1.5 overflow-y-auto">
              {preview.targets.map((t) => <Badge key={t.id} tone={t.online ? "good" : "neutral"}><span className="flex items-center gap-1"><OsIcon os={t.os} className="size-3" />{t.hostname}</span></Badge>)}
            </div>
            {preview.targets.some((t) => !t.online) && <p className="mt-2 text-xs text-ink-3">Offline endpoints run it when they next check in.</p>}
          </div>
        )}
        {n >= 10 && <Callout tone="warn" title={`This runs on ${n} computers at once`}>Scripts run with full system rights. Try it on one endpoint first if you haven't.</Callout>}
      </div>
    </Dialog>
  );
}
