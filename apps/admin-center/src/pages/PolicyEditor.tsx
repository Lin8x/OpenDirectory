import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { ArrowLeft, Globe, Plus, Save, Trash2, X } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { del, post, put } from "../lib/api";
import type { CatalogEntry, Group, OrgUnit, Policy, Script, ScriptTrigger, Setting, SettingKind, SupportLevel } from "../lib/types";
import {
  Badge, Button, Card, Confirm, Dialog, Explain, Field, Input, Loading, OsIcon, Select, Switch, Textarea, cx, useToast,
} from "../components/ui";
import { OuSelect, ouPath } from "./Users";
import { useCaRoot } from "./NetworkServices";

type Draft = Omit<Policy, "id" | "created_at" | "updated_at">;
const EMPTY: Draft = { name: "", description: "", enabled: true, priority: 100, targets: { all_endpoints: true, ou_ids: [] }, settings: [] };

const levelTone: Record<SupportLevel, "good" | "warn" | "neutral"> = { enforced: "good", report_only: "warn", unsupported: "neutral" };
const levelLabel: Record<SupportLevel, string> = { enforced: "Enforced", report_only: "Reports", unsupported: "No" };

export default function PolicyEditor() {
  const { id } = useParams();
  const isNew = id === "new";
  const [params] = useSearchParams();
  const nav = useNavigate();
  const toast = useToast();
  const existing = useQuery<Policy>(isNew ? null : `/policies/${id}`);
  const catalog = useQuery<CatalogEntry[]>("/policies/catalog");
  const groups = useQuery<Group[]>("/groups");
  const scripts = useQuery<Script[]>("/scripts");
  const ous = useQuery<OrgUnit[]>("/ous");
  const [d, setD] = useState<Draft | null>(isNew ? EMPTY : null);
  const [picker, setPicker] = useState(false);
  const [saving, setSaving] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [addOu, setAddOu] = useState<string | null>(null);

  useEffect(() => { if (existing.data) setD(existing.data); }, [existing.data]);
  useEffect(() => {
    if (isNew && params.get("preset") === "signin" && groups.data && d && d.settings.length === 0) {
      // Prefer a group the admin created; fall back to everyone (Domain Users).
      const g = groups.data.find((x) => x.name !== "Domain Admins" && x.name !== "Domain Users") ?? groups.data.find((x) => x.name === "Domain Users") ?? groups.data[0];
      setD({ ...d, name: "Sign-in accounts", description: "Lets members of a security group sign in to endpoints with their directory credentials.", settings: [{ kind: "local_accounts", group_id: g?.id ?? "", admin: false }] });
    }
  }, [isNew, params, groups.data]); // eslint-disable-line react-hooks/exhaustive-deps

  const cat = useMemo(() => new Map((catalog.data ?? []).map((c) => [c.kind, c])), [catalog.data]);
  if (!d || catalog.loading) return <Loading />;

  const setSetting = (i: number, s: Setting) => setD({ ...d, settings: d.settings.map((x, j) => (j === i ? s : x)) });
  const used = new Set<SettingKind>(d.settings.filter((s) => s.kind !== "local_accounts" && s.kind !== "install_packages" && s.kind !== "run_script").map((s) => s.kind));

  const save = async () => {
    setSaving(true);
    try {
      if (isNew) { const p = await post<Policy>("/policies", d); toast("good", "GPO created"); nav(`/gpos/${p.id}`, { replace: true }); }
      else { await put(`/policies/${id}`, d); toast("good", "GPO saved. Endpoints apply it at their next check-in."); void existing.reload(); }
    } catch (e) { toast("bad", (e as Error).message); } finally { setSaving(false); }
  };

  return (
    <div className="space-y-5">
      <Link to="/gpos" className="inline-flex items-center gap-1 text-sm text-ink-3 hover:text-ink"><ArrowLeft className="size-4" />Group Policy</Link>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-[22px] font-semibold tracking-tight">{isNew ? "New Group Policy Object" : d.name || "Untitled GPO"}</h1>
        <div className="flex gap-2">
          {!isNew && <Button variant="ghost" icon={<Trash2 className="size-4" />} onClick={() => setConfirm(true)}>Delete</Button>}
          <Button variant="primary" icon={<Save className="size-4" />} loading={saving} disabled={!d.name.trim()} onClick={save}>{isNew ? "Create GPO" : "Save"}</Button>
        </div>
      </div>
      <Explain term="gpo" />

      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_340px]">
        <div className="space-y-5">
          <Card title="Settings" subtitle="Each setting is applied with the native mechanism on every OS" actions={<Button size="sm" variant="primary" icon={<Plus className="size-3.5" />} onClick={() => setPicker(true)}>Add setting</Button>}>
            {d.settings.length === 0 ? (
              <div className="py-8 text-center">
                <p className="text-sm text-ink-2">This GPO has no settings yet.</p>
                <Button className="mt-3" icon={<Plus className="size-4" />} onClick={() => setPicker(true)}>Add a setting</Button>
              </div>
            ) : (
              <div className="space-y-3">
                {d.settings.map((s, i) => {
                  const c = cat.get(s.kind);
                  return (
                    <div key={i} className="rounded-xl border border-line">
                      <div className="flex items-start justify-between gap-3 border-b border-line px-4 py-3">
                        <div>
                          <div className="flex items-center gap-2"><span className="text-sm font-semibold">{c?.title ?? s.kind}</span><Badge>{c?.category}</Badge></div>
                          <p className="mt-0.5 text-xs leading-relaxed text-ink-3">{c?.summary}</p>
                        </div>
                        <button className="rounded p-1 text-ink-3 hover:bg-panel-2 hover:text-bad" onClick={() => setD({ ...d, settings: d.settings.filter((_, j) => j !== i) })} aria-label="Remove"><X className="size-4" /></button>
                      </div>
                      <div className="px-4 py-3"><SettingForm s={s} onChange={(v) => setSetting(i, v)} groups={groups.data ?? []} scripts={scripts.data ?? []} entry={c} /></div>
                      {c && <SupportStrip c={c} />}
                    </div>
                  );
                })}
              </div>
            )}
          </Card>
        </div>

        <div className="space-y-5">
          <Card title="Details">
            <div className="space-y-3">
              <Field label="Name"><Input value={d.name} onChange={(e) => setD({ ...d, name: e.target.value })} placeholder="Workstation security baseline" autoFocus={isNew} /></Field>
              <Field label="Description"><Textarea rows={3} value={d.description} onChange={(e) => setD({ ...d, description: e.target.value })} /></Field>
              <Field label="Priority" term="priority" hint="Lower wins when GPOs conflict."><Input type="number" value={d.priority} onChange={(e) => setD({ ...d, priority: Number(e.target.value) })} /></Field>
              <Switch checked={d.enabled} onChange={(v) => setD({ ...d, enabled: v })} label="Enabled" />
            </div>
          </Card>
          <Card title="Links" subtitle="Where this GPO applies">
            <div className="space-y-3">
              <Switch checked={d.targets.all_endpoints} onChange={(v) => setD({ ...d, targets: { ...d.targets, all_endpoints: v } })} label={<span className="flex items-center gap-1.5"><Globe className="size-3.5" />All endpoints in the domain</span>} />
              {!d.targets.all_endpoints && (
                <>
                  <div className="space-y-1.5">
                    {d.targets.ou_ids.map((o) => (
                      <div key={o} className="flex items-center justify-between rounded-md bg-panel-2 px-2.5 py-1.5 text-sm">{ouPath(ous.data ?? [], o)}
                        <button className="text-ink-3 hover:text-bad" onClick={() => setD({ ...d, targets: { ...d.targets, ou_ids: d.targets.ou_ids.filter((x) => x !== o) } })}><X className="size-3.5" /></button>
                      </div>
                    ))}
                    {d.targets.ou_ids.length === 0 && <p className="text-xs text-warn">Not linked to any OU yet, so it applies nowhere.</p>}
                  </div>
                  <div className="flex gap-2">
                    <OuSelect ous={(ous.data ?? []).filter((o) => !d.targets.ou_ids.includes(o.id))} value={addOu} onChange={setAddOu} none="Choose an OU…" />
                    <Button disabled={!addOu} onClick={() => { setD({ ...d, targets: { ...d.targets, ou_ids: [...d.targets.ou_ids, addOu!] } }); setAddOu(null); }}>Link</Button>
                  </div>
                  <p className="text-xs text-ink-3">Endpoints in child OUs receive it too.</p>
                </>
              )}
            </div>
          </Card>
        </div>
      </div>

      <Dialog open={picker} onClose={() => setPicker(false)} title="Add a setting" wide>
        <div className="grid gap-3 sm:grid-cols-2">
          {(catalog.data ?? []).map((c) => {
            const disabled = used.has(c.kind);
            return (
              <button key={c.kind} disabled={disabled} onClick={() => { setD({ ...d, settings: [...d.settings, defaultFor(c, groups.data ?? [])] }); setPicker(false); }}
                className={cx("rounded-xl border border-line p-4 text-left transition", disabled ? "opacity-40" : "hover:border-accent hover:shadow-md")}>
                <div className="flex items-center justify-between"><span className="text-sm font-semibold">{c.title}</span><Badge>{c.category}</Badge></div>
                <p className="mt-1 text-xs leading-relaxed text-ink-3">{c.summary}</p>
                <div className="mt-2.5 flex gap-1.5">{c.support.map((p) => <span key={p.os} className="flex items-center gap-1 text-[11px] text-ink-3"><OsIcon os={p.os} className="size-3" />{levelLabel[p.level]}</span>)}</div>
                <div className="mt-1.5 text-[11px] text-ink-3">AD: {c.ad_equivalent}</div>
              </button>
            );
          })}
        </div>
      </Dialog>
      <Confirm open={confirm} onClose={() => setConfirm(false)} danger title={`Delete ${d.name}?`} confirmLabel="Delete GPO"
        body="Endpoints stop receiving these settings. Settings already applied stay as they are on each computer."
        onConfirm={async () => { await del(`/policies/${id}`); toast("good", "GPO deleted"); nav("/gpos"); }} />
    </div>
  );
}

function defaultFor(c: CatalogEntry, groups: Group[]): Setting {
  if (c.kind === "run_script") return { kind: "run_script", script_id: "", trigger: { type: "startup" }, params: {} };
  if (c.kind === "local_accounts") return { kind: "local_accounts", group_id: groups.find((g) => g.name !== "Domain Admins")?.id ?? "", admin: false };
  return JSON.parse(JSON.stringify(c.default)) as Setting;
}

function SupportStrip({ c }: { c: CatalogEntry }) {
  return (
    <div className="grid grid-cols-3 divide-x divide-line rounded-b-xl border-t border-line bg-panel-2/60">
      {c.support.map((p) => (
        <div key={p.os} className="px-3 py-2" title={p.mechanism}>
          <div className="flex items-center gap-1.5 text-xs font-medium"><OsIcon os={p.os} className="size-3.5" /><Badge tone={levelTone[p.level]} className="px-1.5 py-0">{levelLabel[p.level]}</Badge></div>
          <div className="mt-1 truncate text-[11px] text-ink-3">{p.mechanism}</div>
        </div>
      ))}
    </div>
  );
}

function SettingForm({ s, onChange, groups, scripts, entry }: { s: Setting; onChange: (s: Setting) => void; groups: Group[]; scripts: Script[]; entry?: CatalogEntry }) {
  switch (s.kind as SettingKind) {
    case "password_min_length": {
      const v = s as Extract<Setting, { kind: "password_min_length" }>;
      return <Field label="Minimum length (characters)"><Input type="number" min={4} max={128} className="w-32" value={v.length} onChange={(e) => onChange({ ...v, length: Number(e.target.value) })} /></Field>;
    }
    case "screen_lock": {
      const v = s as Extract<Setting, { kind: "screen_lock" }>;
      return <Field label="Lock after (minutes idle)"><Input type="number" min={1} max={240} className="w-32" value={v.minutes} onChange={(e) => onChange({ ...v, minutes: Number(e.target.value) })} /></Field>;
    }
    case "firewall": {
      const v = s as Extract<Setting, { kind: "firewall" }>;
      return <Switch checked={v.enabled} onChange={(x) => onChange({ ...v, enabled: x })} label="Keep the firewall on" />;
    }
    case "disk_encryption": {
      const v = s as Extract<Setting, { kind: "disk_encryption" }>;
      return <Switch checked={v.required} onChange={(x) => onChange({ ...v, required: x })} label="Flag endpoints whose system disk isn't encrypted" />;
    }
    case "automatic_updates": {
      const v = s as Extract<Setting, { kind: "automatic_updates" }>;
      return <Switch checked={v.enabled} onChange={(x) => onChange({ ...v, enabled: x })} label="Install OS updates automatically" />;
    }
    case "local_accounts": {
      const v = s as Extract<Setting, { kind: "local_accounts" }>;
      return (
        <div className="space-y-3">
          <Field label="Security group" term="local_accounts" hint="Every member gets a sign-in account on the endpoints this GPO applies to.">
            <Select value={v.group_id} onChange={(e) => onChange({ ...v, group_id: e.target.value })}>
              <option value="">Choose a group…</option>
              {groups.map((g) => <option key={g.id} value={g.id}>{g.name} ({g.member_ids.length} {g.member_ids.length === 1 ? "member" : "members"})</option>)}
            </Select>
          </Field>
          <Switch checked={v.admin} onChange={(x) => onChange({ ...v, admin: x })} label="Make them local administrators" />
        </div>
      );
    }
    case "login_banner": {
      const v = s as Extract<Setting, { kind: "login_banner" }>;
      return (
        <div className="space-y-3">
          <Field label="Title"><Input value={v.title} onChange={(e) => onChange({ ...v, title: e.target.value })} /></Field>
          <Field label="Message"><Textarea rows={3} value={v.text} onChange={(e) => onChange({ ...v, text: e.target.value })} /></Field>
        </div>
      );
    }
    case "install_packages": {
      const v = s as Extract<Setting, { kind: "install_packages" }>;
      return <PackageList value={v.packages} onChange={(p) => onChange({ ...v, packages: p })} />;
    }
    case "run_script": {
      const v = s as Extract<Setting, { kind: "run_script" }>;
      return <ScriptSettingForm v={v} onChange={onChange} scripts={scripts} />;
    }
    case "trusted_root_certificate": {
      const v = s as Extract<Setting, { kind: "trusted_root_certificate" }>;
      return <TrustedRootForm v={v} onChange={onChange} />;
    }
    default:
      return <GenericSettingForm s={s} onChange={onChange} entry={entry} />;
  }
}

const TRIGGERS: { id: ScriptTrigger["type"]; label: string; hint: string }[] = [
  { id: "startup", label: "At startup", hint: "Each time the computer starts, before anyone signs in." },
  { id: "logon", label: "At sign-in", hint: "Each time someone signs in. The user name is in OD_LOGON_USER." },
  { id: "interval", label: "On a schedule", hint: "Repeatedly, every N minutes." },
  { id: "once", label: "Once", hint: "One time per endpoint, and again whenever you edit the script." },
];

function ScriptSettingForm({ v, onChange, scripts }: { v: Extract<Setting, { kind: "run_script" }>; onChange: (s: Setting) => void; scripts: Script[] }) {
  const sc = scripts.find((x) => x.id === v.script_id);
  if (scripts.length === 0) return <p className="text-sm text-ink-3">Your Script Library is empty. <Link to="/scripts?tab=templates" className="text-accent hover:underline">Add a script or template</Link> first.</p>;
  return (
    <div className="space-y-3">
      <Field label="Script">
        <Select value={v.script_id} onChange={(e) => { const n = scripts.find((x) => x.id === e.target.value); onChange({ ...v, script_id: e.target.value, params: Object.fromEntries((n?.params ?? []).map((p) => [p.name, p.default])) }); }}>
          <option value="">Choose a script…</option>
          {scripts.map((x) => <option key={x.id} value={x.id}>{x.name}</option>)}
        </Select>
      </Field>
      <Field label="When it runs" hint={TRIGGERS.find((t) => t.id === v.trigger.type)?.hint}>
        <div className="flex flex-wrap items-center gap-2">
          <Select value={v.trigger.type} className="w-48" onChange={(e) => {
            const t = e.target.value as ScriptTrigger["type"];
            onChange({ ...v, trigger: t === "interval" ? { type: "interval", minutes: 60 } : ({ type: t } as ScriptTrigger) });
          }}>{TRIGGERS.map((t) => <option key={t.id} value={t.id}>{t.label}</option>)}</Select>
          {v.trigger.type === "interval" && (
            <span className="flex items-center gap-1.5 text-sm">every<Input type="number" min={5} max={10080} className="w-24" value={v.trigger.minutes} onChange={(e) => onChange({ ...v, trigger: { type: "interval", minutes: Number(e.target.value) } })} />minutes</span>
          )}
        </div>
      </Field>
      {sc && sc.params.length > 0 && (
        <div className="grid gap-3 sm:grid-cols-2">
          {sc.params.map((p) => <Field key={p.name} label={`${p.label || p.name}${p.required ? " *" : ""}`}><Input value={v.params[p.name] ?? p.default} onChange={(e) => onChange({ ...v, params: { ...v.params, [p.name]: e.target.value } })} /></Field>)}
        </div>
      )}
      {sc && sc.os.length > 0 && <p className="text-xs text-ink-3">Runs only on {sc.os.map((o) => (o === "macos" ? "macOS" : o[0]!.toUpperCase() + o.slice(1))).join(", ")} endpoints. <Link to={`/scripts/${sc.id}`} className="text-accent hover:underline">Open script</Link></p>}
    </div>
  );
}

/** Fallback editor for settings without a dedicated form: one input per field. */
const LABELS: Record<string, string> = {
  enabled: "On", required: "Required", allowed: "Allowed", threshold: "Wrong passwords before lockout", duration_minutes: "Lockout duration (minutes)",
  days: "Days (0 = never)", count: "Passwords remembered", server: "Time server", quality_days: "Quality update delay (days)", feature_days: "Feature update delay (days)", mode: "Mode",
};

function GenericSettingForm({ s, onChange, entry }: { s: Setting; onChange: (s: Setting) => void; entry?: CatalogEntry }) {
  const fields = Object.entries(s).filter(([k]) => k !== "kind");
  if (fields.length === 0) return <p className="text-sm text-ink-3">No options. Adding it turns it on.</p>;
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      {fields.map(([k, val]) => {
        const label = LABELS[k] ?? k.replace(/_/g, " ").replace(/^./, (c) => c.toUpperCase());
        const set = (x: unknown) => onChange({ ...s, [k]: x } as Setting);
        const choice = entry?.choices?.find((c) => c.field === k);
        if (choice) return <Field key={k} label={label}><Select value={String(val)} onChange={(e) => set(e.target.value)}>{choice.options.map(([v, l]) => <option key={v} value={v}>{l}</option>)}</Select></Field>;
        if (typeof val === "boolean") return <div key={k} className="pt-6"><Switch checked={val} onChange={set} label={label} /></div>;
        if (typeof val === "number") return <Field key={k} label={label}><Input type="number" value={val} onChange={(e) => set(Number(e.target.value))} /></Field>;
        if (Array.isArray(val)) return <div key={k} className="sm:col-span-2"><Field label={label}><PackageList value={val as string[]} onChange={set} /></Field></div>;
        return <Field key={k} label={label}><Input value={String(val)} onChange={(e) => set(e.target.value)} /></Field>;
      })}
    </div>
  );
}

function PackageList({ value, onChange }: { value: string[]; onChange: (v: string[]) => void }) {
  const [text, setText] = useState("");
  const add = () => { const t = text.trim(); if (t && !value.includes(t)) onChange([...value, t]); setText(""); };
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap gap-1.5">
        {value.map((p) => <span key={p} className="inline-flex items-center gap-1 rounded-full bg-panel-2 px-2.5 py-1 font-mono text-xs">{p}<button onClick={() => onChange(value.filter((x) => x !== p))} className="text-ink-3 hover:text-bad"><X className="size-3" /></button></span>)}
      </div>
      <div className="flex gap-2">
        <Input value={text} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => e.key === "Enter" && add()} placeholder="Mozilla.Firefox (winget), firefox (apt/brew)…" className="font-mono text-xs" />
        <Button onClick={add}>Add</Button>
      </div>
      <p className="text-xs text-ink-3">Names are passed to each OS's package manager; a name that doesn't exist on an OS is reported as failed there.</p>
    </div>
  );
}

function TrustedRootForm({ v, onChange }: { v: Extract<Setting, { kind: "trusted_root_certificate" }>; onChange: (s: Setting) => void }) {
  const ca = useCaRoot();
  return (
    <div className="space-y-3">
      <Field label="Name"><Input value={v.name} onChange={(e) => onChange({ ...v, name: e.target.value })} /></Field>
      <Field label="Certificate (PEM)"><Textarea rows={5} className="font-mono text-[11px]" value={v.pem} placeholder="-----BEGIN CERTIFICATE-----" onChange={(e) => onChange({ ...v, pem: e.target.value })} /></Field>
      {ca.data?.initialized && <Button size="sm" onClick={() => onChange({ ...v, name: ca.data!.common_name ?? v.name, pem: ca.data!.root_pem ?? "" })}>Use this domain's certificate authority</Button>}
    </div>
  );
}
