// Onboarding: pick a security baseline, layer modifiers on top, and review
// exactly what will be created. The plan comes from the Management Server, so
// the preview is what setup really does.
import { useEffect, useState } from "react";
import { AlertTriangle, Building, Check, Clock, FolderTree, GraduationCap, Home, Info, Lock, Shield, ShieldCheck, Sparkles, Square, UsersRound } from "lucide-react";
import { get, post } from "../lib/api";
import { Badge, Field, Input, Select, cx } from "./ui";

export type BaselineId = "custom" | "minimal" | "home" | "small_business" | "enterprise" | "education" | "high_security";
export type Level = "relaxed" | "balanced" | "strict" | "maximum";
export interface BaselineInfo { id: BaselineId; name: string; tagline: string; description: string; best_for: string; default_level: Level | null; not_yet: string[] }
export interface Catalog { baselines: BaselineInfo[]; levels: [Level, string][] }
export interface Modifiers {
  security_level?: Level; education?: "k12" | "higher_ed"; updates?: "automatic" | "rings" | "manual";
  ownership?: "corporate" | "byod" | "mixed"; admin_model?: "single" | "delegated" | "tiered"; local_admin?: boolean;
  remote_access?: "off" | "unmanaged" | "on"; usb?: "allow" | "read_only" | "block"; audit_retention_days?: number;
  compliance: string[]; naming_convention?: string; time_server?: string; rollout?: "everyone" | "pilot" | "review";
}
interface Plan {
  level: Level | null; ous: { name: string; description: string; parent: string | null }[]; groups: { name: string; description: string }[];
  policies: { name: string; description: string; priority: number; enabled: boolean; all_endpoints: boolean; ou_links: string[]; settings: { kind: string }[] }[];
  signin_group: string | null; signin_admin: boolean; audit_retention_days: number | null; notes: { kind: "info" | "warning" | "not_yet"; text: string }[];
}

const ICON: Record<BaselineId, React.ReactNode> = {
  custom: <Square className="size-4" />, minimal: <Sparkles className="size-4" />, home: <Home className="size-4" />, small_business: <Building className="size-4" />,
  enterprise: <UsersRound className="size-4" />, education: <GraduationCap className="size-4" />, high_security: <Lock className="size-4" />,
};
const LEVELS: { id: Level; label: string }[] = [{ id: "relaxed", label: "Relaxed" }, { id: "balanced", label: "Balanced" }, { id: "strict", label: "Strict" }, { id: "maximum", label: "Maximum" }];
const PACKS: [string, string, string][] = [
  ["hipaa", "HIPAA", "US healthcare"], ["pci_dss", "PCI DSS", "Card payments"], ["soc2", "SOC 2", "Service providers"], ["cmmc", "CMMC", "US defense contractors"],
  ["gdpr", "GDPR", "EU personal data"], ["ferpa", "FERPA", "US student records"], ["cipa", "CIPA", "US schools and libraries"],
];

export function useBaselineCatalog() {
  const [c, setC] = useState<Catalog | null>(null);
  useEffect(() => { get<Catalog>("/baselines").then(setC).catch(() => {}); }, []);
  return c;
}

// ------------------------------------------------------------------ step 1
const QUIZ = [
  { q: "Who is this for?", a: [["Me, my family or my lab", "home"], ["A business", "biz"], ["A school or university", "education"]] },
  { q: "How many people?", a: [["Under 250", "small"], ["250 or more, or several sites", "large"]] },
  { q: "Do you handle sensitive or regulated data?", a: [["No, or not much", "no"], ["Yes: health, payments, defense or similar", "yes"]] },
] as const;

function recommend(answers: string[]): BaselineId {
  if (answers[0] === "home") return "home";
  if (answers[0] === "education") return "education";
  if (answers[2] === "yes") return "high_security";
  return answers[1] === "large" ? "enterprise" : "small_business";
}

export function BaselineStep({ catalog, value, onChange }: { catalog: Catalog; value: BaselineId; onChange: (b: BaselineId) => void }) {
  const [quiz, setQuiz] = useState<string[] | null>(null);
  const qi = quiz?.length ?? 0;
  return (
    <div className="space-y-3">
      {quiz === null ? (
        <button onClick={() => setQuiz([])} className="text-sm text-accent hover:underline">Not sure? Answer three questions and we'll recommend one.</button>
      ) : qi < QUIZ.length && !(qi === 1 && quiz[0] !== "biz") ? (
        <div className="rounded-xl border border-accent/40 bg-accent-soft/40 p-3">
          <div className="text-sm font-medium">{QUIZ[qi]!.q}</div>
          <div className="mt-2 flex flex-wrap gap-2">
            {QUIZ[qi]!.a.map(([label, v]) => (
              <button key={v} onClick={() => {
                const next = [...quiz, v];
                if (next[0] !== "biz" && next.length === 1) { onChange(recommend(next)); setQuiz([...next, "", ""]); return; }
                if (next.length === QUIZ.length) onChange(recommend(next));
                setQuiz(next);
              }} className="rounded-full border border-line bg-panel px-3 py-1 text-xs hover:border-accent">{label}</button>
            ))}
          </div>
        </div>
      ) : (
        <div className="flex items-center gap-2 text-sm"><Check className="size-4 text-good" />We recommend <b>{catalog.baselines.find((b) => b.id === value)?.name}</b>. You can still pick another.<button className="text-accent hover:underline" onClick={() => setQuiz([])}>Start over</button></div>
      )}
      <div className="grid gap-2 sm:grid-cols-2">
        {catalog.baselines.map((b) => (
          <button key={b.id} onClick={() => onChange(b.id)}
            className={cx("rounded-xl border p-3 text-left transition", value === b.id ? "border-accent bg-accent-soft/50 ring-1 ring-accent" : "border-line hover:border-accent/60")}>
            <div className="flex items-center gap-2">
              <span className={cx("grid size-7 place-items-center rounded-lg", value === b.id ? "bg-accent text-accent-ink" : "bg-panel-2 text-ink-2")}>{ICON[b.id]}</span>
              <span className="text-sm font-semibold">{b.name}</span>
              {b.id === "small_business" && <Badge tone="accent">Common</Badge>}
            </div>
            <p className="mt-1.5 text-xs leading-relaxed text-ink-2">{b.description}</p>
            <p className="mt-1 text-[11px] text-ink-3">Best for: {b.best_for}</p>
          </button>
        ))}
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ step 2
export function ModifiersStep({ catalog, baseline, value, onChange }: { catalog: Catalog; baseline: BaselineId; value: Modifiers; onChange: (m: Modifiers) => void }) {
  const info = catalog.baselines.find((b) => b.id === baseline)!;
  const level = value.security_level ?? info.default_level;
  const set = <K extends keyof Modifiers>(k: K, v: Modifiers[K]) => onChange({ ...value, [k]: v });
  const enforces = baseline !== "custom" && baseline !== "minimal";
  return (
    <div className="space-y-5">
      {enforces && level && (
        <div>
          <div className="mb-1.5 flex items-center justify-between"><span className="text-sm font-medium">Security level</span>{info.default_level && level !== info.default_level && <button className="text-xs text-accent hover:underline" onClick={() => set("security_level", undefined)}>Reset to baseline ({info.default_level})</button>}</div>
          <div className="grid grid-cols-4 gap-1 rounded-lg bg-panel-2 p-1">
            {LEVELS.map((l) => <button key={l.id} onClick={() => set("security_level", l.id)} className={cx("rounded-md py-1.5 text-xs font-medium", level === l.id ? "bg-panel text-ink shadow" : "text-ink-3 hover:text-ink")}>{l.label}</button>)}
          </div>
          <p className="mt-1.5 text-xs text-ink-3">{catalog.levels.find(([l]) => l === level)?.[1]}. Higher is safer and less convenient.</p>
        </div>
      )}
      <div className="grid gap-4 sm:grid-cols-2">
        {baseline === "education" && (
          <Field label="School type"><Select value={value.education ?? "k12"} onChange={(e) => set("education", e.target.value as Modifiers["education"])}><option value="k12">K-12 (stricter, school-owned devices)</option><option value="higher_ed">Higher Education (BYOD-friendly, research OU)</option></Select></Field>
        )}
        {enforces && <Field label="Operating system updates"><Select value={value.updates ?? ""} onChange={(e) => set("updates", (e.target.value || undefined) as Modifiers["updates"])}><option value="">Baseline default</option><option value="automatic">Automatic</option><option value="rings">Patch rings (pilot first, others wait)</option><option value="manual">Manual (not managed)</option></Select></Field>}
        {enforces && <Field label="Who owns the devices"><Select value={value.ownership ?? ""} onChange={(e) => set("ownership", (e.target.value || undefined) as Modifiers["ownership"])}><option value="">Baseline default</option><option value="corporate">The organization</option><option value="byod">Employees (BYOD)</option><option value="mixed">Both</option></Select></Field>}
        <Field label="Admin model" hint="Tiered keeps identity admins separate from workstation admins."><Select value={value.admin_model ?? ""} onChange={(e) => set("admin_model", (e.target.value || undefined) as Modifiers["admin_model"])}><option value="">Baseline default</option><option value="single">Single admin team</option><option value="delegated">Delegated (help desk + admins)</option><option value="tiered">Tiered (Tier 0, 1, 2)</option></Select></Field>
        {enforces && <Field label="Signed-in users are local admins"><Select value={value.local_admin === undefined ? "" : String(value.local_admin)} onChange={(e) => set("local_admin", e.target.value === "" ? undefined : e.target.value === "true")}><option value="">Baseline default</option><option value="false">No (standard users)</option><option value="true">Yes</option></Select></Field>}
        {enforces && <Field label="Remote access (RDP, SSH)"><Select value={value.remote_access ?? ""} onChange={(e) => set("remote_access", (e.target.value || undefined) as Modifiers["remote_access"])}><option value="">Baseline default</option><option value="off">Off everywhere</option><option value="unmanaged">Leave as the OS ships it</option><option value="on">Remote Desktop on</option></Select></Field>}
        {enforces && <Field label="USB storage"><Select value={value.usb ?? ""} onChange={(e) => set("usb", (e.target.value || undefined) as Modifiers["usb"])}><option value="">Baseline default</option><option value="allow">Allow</option><option value="read_only">Read-only</option><option value="block">Block</option></Select></Field>}
        <Field label="Audit log retention"><Select value={value.audit_retention_days ?? ""} onChange={(e) => set("audit_retention_days", e.target.value ? Number(e.target.value) : undefined)}><option value="">Keep forever</option><option value="30">30 days</option><option value="90">90 days</option><option value="365">1 year</option><option value="2555">7 years</option></Select></Field>
        {enforces && <Field label="Time server (NTP)" hint="Leave blank to keep each OS's default."><Input value={value.time_server ?? ""} placeholder="time.windows.com or pool.ntp.org" onChange={(e) => set("time_server", e.target.value || undefined)} /></Field>}
        <Field label="Computer naming convention" hint="For reference when you name new computers."><Input value={value.naming_convention ?? ""} placeholder="LAPTOP-%USER%-###" onChange={(e) => set("naming_convention", e.target.value || undefined)} /></Field>
      </div>
      {enforces && (
        <div>
          <div className="mb-1.5 text-sm font-medium">Compliance packs</div>
          <div className="flex flex-wrap gap-1.5">
            {PACKS.map(([id, label, hint]) => {
              const on = value.compliance.includes(id);
              return <button key={id} title={hint} onClick={() => set("compliance", on ? value.compliance.filter((x) => x !== id) : [...value.compliance, id])}
                className={cx("rounded-full border px-3 py-1 text-xs", on ? "border-accent bg-accent-soft text-accent" : "border-line text-ink-2 hover:border-accent/60")}>{on && <Check className="mr-1 inline size-3" />}{label}</button>;
            })}
          </div>
          <p className="mt-1.5 text-xs text-ink-3">Packs tighten settings and keep audit logs longer. They help with the technical controls; they don't certify you.</p>
        </div>
      )}
      {enforces && (
        <Field label="How to roll it out">
          <div className="grid gap-2 sm:grid-cols-3">
            {([["everyone", "Everyone now", "Applies as soon as computers enroll."], ["pilot", "Pilot OU first", "Only computers in the Pilot OU get it until you widen it."], ["review", "Review first", "GPOs are created turned off."]] as const).map(([id, t, d]) => (
              <button key={id} onClick={() => set("rollout", id)} className={cx("rounded-lg border p-2.5 text-left", (value.rollout ?? "everyone") === id ? "border-accent bg-accent-soft/50" : "border-line")}>
                <div className="text-xs font-semibold">{t}</div><div className="mt-0.5 text-[11px] text-ink-3">{d}</div>
              </button>
            ))}
          </div>
        </Field>
      )}
    </div>
  );
}

// ------------------------------------------------------------------ step 3
export function ReviewStep({ baseline, modifiers }: { baseline: BaselineId; modifiers: Modifiers }) {
  const [plan, setPlan] = useState<Plan | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => { post<Plan>("/baselines/preview", { baseline, modifiers }).then(setPlan).catch((e) => setErr((e as Error).message)); }, [baseline, modifiers]);
  if (err) return <p className="text-sm text-bad">{err}</p>;
  if (!plan) return <p className="text-sm text-ink-3">Working out the plan…</p>;
  const tone = { info: "text-ink-2", warning: "text-warn", not_yet: "text-ink-3" } as const;
  const icon = { info: <Info className="size-3.5" />, warning: <AlertTriangle className="size-3.5" />, not_yet: <Clock className="size-3.5" /> } as const;
  return (
    <div className="max-h-[52vh] space-y-4 overflow-y-auto pr-1 text-sm">
      {plan.notes.length > 0 && (
        <ul className="space-y-1.5">{plan.notes.map((n, i) => <li key={i} className={cx("flex gap-2 text-xs leading-relaxed", tone[n.kind])}><span className="mt-0.5 shrink-0">{icon[n.kind]}</span>{n.text}</li>)}</ul>
      )}
      <Section icon={<FolderTree className="size-4" />} title={`${plan.ous.length} organizational units`}>
        {plan.ous.length === 0 ? <None /> : <div className="flex flex-wrap gap-1.5">{plan.ous.map((o) => <Badge key={o.name}>{o.parent ? `${o.parent} / ` : ""}{o.name}</Badge>)}</div>}
      </Section>
      <Section icon={<UsersRound className="size-4" />} title={`${plan.groups.length + 2} security groups`}>
        <div className="flex flex-wrap gap-1.5"><Badge>Domain Admins</Badge><Badge>Domain Users</Badge>{plan.groups.map((g) => <Badge key={g.name}>{g.name}</Badge>)}</div>
      </Section>
      <Section icon={<ShieldCheck className="size-4" />} title={`${plan.policies.length + (plan.signin_group ? 1 : 0)} Group Policy Objects`}>
        {plan.policies.length === 0 && !plan.signin_group ? <None /> : (
          <ul className="space-y-2">
            {plan.policies.map((p) => (
              <li key={p.name} className="rounded-lg border border-line p-2.5">
                <div className="flex items-center gap-2"><span className="font-medium">{p.name}</span>{!p.enabled && <Badge tone="warn">Off</Badge>}<span className="text-xs text-ink-3">{p.all_endpoints ? "All endpoints" : p.ou_links.join(", ")}</span></div>
                <div className="mt-1 text-xs text-ink-3">{p.settings.length} settings: {p.settings.map((s) => s.kind.replace(/_/g, " ")).join(", ")}</div>
              </li>
            ))}
            {plan.signin_group && <li className="rounded-lg border border-line p-2.5"><span className="font-medium">Sign-in Accounts</span><div className="mt-1 text-xs text-ink-3">Members of {plan.signin_group} can sign in to managed computers{plan.signin_admin ? " as local administrators" : ""}.</div></li>}
          </ul>
        )}
      </Section>
      {plan.audit_retention_days && <p className="flex items-center gap-2 text-xs text-ink-3"><Shield className="size-3.5" />Audit log kept {plan.audit_retention_days} days.</p>}
    </div>
  );
}

function Section({ icon, title, children }: { icon: React.ReactNode; title: string; children: React.ReactNode }) {
  return <div><div className="mb-1.5 flex items-center gap-2 font-medium">{icon}{title}</div>{children}</div>;
}
function None() { return <p className="text-xs text-ink-3">None</p>; }
