// Default Domain Password Policy and fine-grained password policies (PSOs).
import { useEffect, useState } from "react";
import { KeyRound, Plus, Trash2 } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { del, post, put } from "../lib/api";
import type { Group } from "../lib/types";
import { Badge, Button, Card, Confirm, Dialog, Explain, Field, Input, PageHeader, Switch, Textarea, cx, useToast } from "../components/ui";

interface Rules { min_length: number; complexity: boolean; history: number; max_age_days: number; lockout_threshold: number; lockout_minutes: number }
interface Pso { id: string; name: string; description: string; precedence: number; group_ids: string[]; rules: Rules }
const DEFAULT: Rules = { min_length: 12, complexity: false, history: 5, max_age_days: 0, lockout_threshold: 5, lockout_minutes: 15 };

function RulesForm({ r, set }: { r: Rules; set: (r: Rules) => void }) {
  const n = (k: keyof Rules) => (e: React.ChangeEvent<HTMLInputElement>) => set({ ...r, [k]: Number(e.target.value) });
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      <Field label="Minimum length"><Input type="number" min={4} max={128} value={r.min_length} onChange={n("min_length")} /></Field>
      <Field label="Remember previous passwords" hint="0 to 24"><Input type="number" min={0} max={24} value={r.history} onChange={n("history")} /></Field>
      <Field label="Maximum age (days)" hint="0 = never expires (NIST recommends this with long passwords)"><Input type="number" min={0} max={999} value={r.max_age_days} onChange={n("max_age_days")} /></Field>
      <div className="pt-6"><Switch checked={r.complexity} onChange={(v) => set({ ...r, complexity: v })} label="Require mixed character types" /></div>
      <Field label="Lockout after wrong passwords" hint="0 = never lock"><Input type="number" min={0} value={r.lockout_threshold} onChange={n("lockout_threshold")} /></Field>
      <Field label="Lockout duration (minutes)"><Input type="number" min={1} value={r.lockout_minutes} onChange={n("lockout_minutes")} /></Field>
    </div>
  );
}

export default function PasswordPolicies() {
  const toast = useToast();
  const domain = useQuery<Rules>("/password-policy");
  const psos = useQuery<Pso[]>("/password-policies");
  const groups = useQuery<Group[]>("/groups");
  const [d, setD] = useState<Rules | null>(null);
  const [edit, setEdit] = useState<Pso | "new" | null>(null);
  const [del_, setDel] = useState<Pso | null>(null);
  useEffect(() => { if (domain.data) setD(domain.data); }, [domain.data]);
  return (
    <>
      <PageHeader title="Password Policies" guide="02-directory#users-one-account-per-person" term="pso" subtitle="Rules for directory passwords: the Default Domain Password Policy, plus stricter or looser policies for specific security groups."
        actions={<Button variant="primary" icon={<Plus className="size-4" />} onClick={() => setEdit("new")}>New password policy</Button>} />
      <div className="grid gap-5 lg:grid-cols-2">
        <Card title="Default Domain Password Policy" subtitle="Applies to everyone not covered by a fine-grained policy">
          {d && <div className="space-y-4"><RulesForm r={d} set={setD} />
            <Button variant="primary" disabled={JSON.stringify(d) === JSON.stringify(domain.data)} onClick={async () => { try { await put("/password-policy", d); toast("good", "Saved"); void domain.reload(); } catch (e) { toast("bad", (e as Error).message); } }}>Save</Button></div>}
        </Card>
        <Card title="Fine-grained password policies" subtitle="Lower precedence wins when someone is in several groups" padded={false}>
          {(psos.data ?? []).length === 0 ? <p className="p-4 text-sm text-ink-3">None yet. A common one: 16+ characters for Domain Admins.</p> : (
            <ul className="divide-y divide-line">
              {psos.data!.map((p) => (
                <li key={p.id} className="flex items-center gap-3 px-4 py-3">
                  <span className="grid size-8 place-items-center rounded-lg bg-accent-soft text-accent"><KeyRound className="size-4" /></span>
                  <button className="min-w-0 flex-1 text-left" onClick={() => setEdit(p)}>
                    <div className="flex items-center gap-2"><span className="font-medium">{p.name}</span><Badge>Precedence {p.precedence}</Badge></div>
                    <div className="truncate text-xs text-ink-3">{p.rules.min_length}+ characters{p.rules.complexity ? ", complex" : ""} · {p.group_ids.map((g) => groups.data?.find((x) => x.id === g)?.name).filter(Boolean).join(", ") || "no groups"}</div>
                  </button>
                  <Button size="sm" variant="ghost" icon={<Trash2 className="size-3.5" />} onClick={() => setDel(p)} aria-label="Delete" />
                </li>
              ))}
            </ul>
          )}
        </Card>
      </div>
      <div className="mt-5"><Explain>These rules apply when passwords are set in OpenDirectory: by an admin, at self-service change, or with a reset code. Local password rules on endpoints are set with GPO settings such as Minimum password length and Account lockout.</Explain></div>
      {edit && <PsoDialog pso={edit === "new" ? null : edit} groups={groups.data ?? []} onClose={() => setEdit(null)} onSaved={() => { setEdit(null); void psos.reload(); }} />}
      <Confirm open={!!del_} onClose={() => setDel(null)} danger title={`Delete ${del_?.name}?`} confirmLabel="Delete" body="Its members fall back to the Default Domain Password Policy."
        onConfirm={async () => { if (del_) { await del(`/password-policies/${del_.id}`); void psos.reload(); } }} />
    </>
  );
}

function PsoDialog({ pso, groups, onClose, onSaved }: { pso: Pso | null; groups: Group[]; onClose: () => void; onSaved: () => void }) {
  const toast = useToast();
  const [p, setP] = useState<Omit<Pso, "id">>(pso ?? { name: "", description: "", precedence: 10, group_ids: [], rules: { ...DEFAULT, min_length: 16, complexity: true, history: 12 } });
  return (
    <Dialog open onClose={onClose} wide title={pso ? `Edit ${pso.name}` : "New password policy"}
      footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!p.name.trim()} onClick={async () => {
        try { if (pso) await put(`/password-policies/${pso.id}`, p); else await post("/password-policies", p); toast("good", "Saved"); onSaved(); } catch (e) { toast("bad", (e as Error).message); }
      }}>Save</Button></>}>
      <div className="space-y-4">
        <div className="grid gap-3 sm:grid-cols-[1fr_140px]">
          <Field label="Name"><Input value={p.name} onChange={(e) => setP({ ...p, name: e.target.value })} placeholder="Privileged accounts" /></Field>
          <Field label="Precedence"><Input type="number" value={p.precedence} onChange={(e) => setP({ ...p, precedence: Number(e.target.value) })} /></Field>
        </div>
        <Field label="Description"><Textarea rows={2} value={p.description} onChange={(e) => setP({ ...p, description: e.target.value })} /></Field>
        <Field label="Applies to these security groups">
          <div className="flex flex-wrap gap-1.5">{groups.map((g) => { const on = p.group_ids.includes(g.id); return (
            <button key={g.id} onClick={() => setP({ ...p, group_ids: on ? p.group_ids.filter((x) => x !== g.id) : [...p.group_ids, g.id] })}
              className={cx("rounded-full border px-3 py-1 text-xs", on ? "border-accent bg-accent-soft text-accent" : "border-line text-ink-2")}>{g.name}</button>); })}</div>
        </Field>
        <RulesForm r={p.rules} set={(r) => setP({ ...p, rules: r })} />
      </div>
    </Dialog>
  );
}
