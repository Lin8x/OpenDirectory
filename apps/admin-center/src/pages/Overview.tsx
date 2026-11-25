import type { ReactNode } from "react";
import { Link } from "react-router-dom";
import { ArrowRight, CheckCircle2, Circle, Monitor, ShieldAlert, ShieldCheck, Terminal, Users, UsersRound, Wifi } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { useSession } from "../lib/session";
import type { Overview as OverviewT } from "../lib/types";
import { ago } from "../lib/format";
import { Card, ErrorNote, Explain, Loading, OsIcon, cx } from "../components/ui";

const STEPS: { key: string; title: string; body: string; to: string }[] = [
  { key: "created_user", title: "Add your users", body: "Create accounts for the people in your organization.", to: "/users?new=1" },
  { key: "created_group", title: "Create a security group", body: "Groups decide who can sign in where.", to: "/groups?new=1" },
  { key: "enrolled_endpoint", title: "Enroll your first endpoint", body: "Install the Endpoint Agent on a Windows, Mac or Linux computer.", to: "/enroll" },
  { key: "configured_sign_in", title: "Let a group sign in to computers", body: "A GPO with Sign-in accounts gives directory users logins on endpoints.", to: "/gpos/new?preset=signin" },
  { key: "created_policy", title: "Create your own GPO", body: "Screen lock, firewall, software and more, on every OS.", to: "/gpos/new" },
  { key: "ran_script", title: "Run a remote script", body: "Check or fix something on an endpoint without leaving your desk.", to: "/scripts" },
];

export default function Overview() {
  const { me } = useSession();
  const { data, error, loading, reload } = useQuery<OverviewT>("/overview", { poll: 10000 });
  if (loading && !data) return <Loading />;
  if (error && !data) return <ErrorNote message={error} onRetry={reload} />;
  if (!data) return null;
  const c = data.counts;
  const done = STEPS.filter((s) => data.getting_started[s.key]).length;
  const hour = new Date().getHours();
  const greet = hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening";

  return (
    <div className="space-y-5">
      <div>
        <h1 className="text-[22px] font-semibold tracking-tight">{greet}, {me?.user.display_name.split(" ")[0]}</h1>
        <p className="mt-1 text-sm text-ink-2">{data.organization} · {data.domain}</p>
      </div>
      <Explain term="management_server" />

      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <Kpi to="/users" icon={<Users className="size-4" />} label="Users" value={c.users} sub={`${c.users_enabled} enabled · ${c.admins} Domain Admin${c.admins === 1 ? "" : "s"}`} />
        <Kpi to="/groups" icon={<UsersRound className="size-4" />} label="Security groups" value={c.groups} sub={`${c.ous} organizational units`} />
        <Kpi to="/endpoints" icon={<Monitor className="size-4" />} label="Endpoints" value={c.endpoints} sub={<span className="flex items-center gap-1.5"><Wifi className="size-3" />{c.endpoints_online} online now</span>} />
        <Kpi to="/endpoints?filter=noncompliant" icon={c.endpoints_noncompliant ? <ShieldAlert className="size-4" /> : <ShieldCheck className="size-4" />} label="Need attention"
          value={c.endpoints_noncompliant} tone={c.endpoints_noncompliant ? "warn" : "good"} sub={c.endpoints_noncompliant ? "Non-compliant endpoints" : "All endpoints compliant"} />
      </div>

      <div className="grid gap-5 lg:grid-cols-[1.35fr_1fr]">
        {done < STEPS.length ? (
          <Card title="Get started" subtitle={`${done} of ${STEPS.length} done`} actions={<Progress value={done / STEPS.length} />}>
            <ol className="-my-1 divide-y divide-line">
              {STEPS.map((s) => {
                const ok = data.getting_started[s.key];
                return (
                  <li key={s.key}>
                    <Link to={s.to} className="group flex items-center gap-3 py-2.5">
                      {ok ? <CheckCircle2 className="size-5 shrink-0 text-good" /> : <Circle className="size-5 shrink-0 text-line" />}
                      <div className="min-w-0 flex-1">
                        <div className={cx("text-sm font-medium", ok && "text-ink-3 line-through")}>{s.title}</div>
                        <div className="text-xs text-ink-3">{s.body}</div>
                      </div>
                      {!ok && <ArrowRight className="size-4 text-ink-3 transition group-hover:translate-x-0.5 group-hover:text-accent" />}
                    </Link>
                  </li>
                );
              })}
            </ol>
          </Card>
        ) : (
          <Card title="Endpoint fleet">
            <FleetBars byOs={data.endpoints_by_os} total={c.endpoints} />
          </Card>
        )}

        <Card title="Recent activity" actions={<Link to="/audit" className="text-xs font-medium text-accent hover:underline">Audit log</Link>} padded={false}>
          {data.recent_activity.length === 0 ? <p className="p-4 text-sm text-ink-3">Nothing yet.</p> : (
            <ul className="divide-y divide-line">
              {data.recent_activity.map((a) => (
                <li key={a.id} className="flex items-start gap-3 px-4 py-2.5">
                  <span className="mt-1.5 size-1.5 shrink-0 rounded-full bg-accent" />
                  <div className="min-w-0 flex-1 text-sm">
                    <span className="font-medium">{a.actor}</span> <span className="text-ink-2">{describe(a.action)}</span> <span className="font-medium">{a.target}</span>
                    {a.details && <div className="truncate text-xs text-ink-3">{a.details}</div>}
                  </div>
                  <span className="shrink-0 text-xs text-ink-3">{ago(a.at)}</span>
                </li>
              ))}
            </ul>
          )}
        </Card>
      </div>

      {done < STEPS.length && c.endpoints > 0 && <Card title="Endpoint fleet"><FleetBars byOs={data.endpoints_by_os} total={c.endpoints} /></Card>}

      <div className="grid gap-4 sm:grid-cols-3">
        <Mini icon={<ShieldCheck className="size-4" />} label="Group Policy Objects" value={c.policies} to="/gpos" />
        <Mini icon={<Terminal className="size-4" />} label="Scripts queued or running" value={c.jobs_running} to="/scripts" />
        <Mini icon={<Monitor className="size-4" />} label="Stale endpoints (14+ days)" value={c.endpoints_stale} to="/endpoints" />
      </div>
    </div>
  );
}

function Kpi({ icon, label, value, sub, to, tone }: { icon: ReactNode; label: string; value: number; sub: ReactNode; to: string; tone?: "good" | "warn" }) {
  return (
    <Link to={to} className="group rounded-xl border border-line bg-panel p-4 shadow-[0_1px_2px_rgba(16,24,40,.04)] transition hover:border-accent/50 hover:shadow-md">
      <div className="flex items-center justify-between text-ink-3">
        <span className="text-[13px] font-medium">{label}</span>
        <span className={cx("grid size-7 place-items-center rounded-lg", tone === "warn" ? "bg-warn-soft text-warn" : tone === "good" ? "bg-good-soft text-good" : "bg-accent-soft text-accent")}>{icon}</span>
      </div>
      <div className="mt-2 text-[28px] font-semibold leading-none tracking-tight">{value}</div>
      <div className="mt-2 text-xs text-ink-3">{sub}</div>
    </Link>
  );
}

function Mini({ icon, label, value, to }: { icon: ReactNode; label: string; value: number; to: string }) {
  return (
    <Link to={to} className="flex items-center gap-3 rounded-xl border border-line bg-panel px-4 py-3 hover:border-accent/50">
      <span className="text-ink-3">{icon}</span><span className="flex-1 text-sm text-ink-2">{label}</span><span className="text-lg font-semibold">{value}</span>
    </Link>
  );
}

function Progress({ value }: { value: number }) {
  return <div className="h-1.5 w-28 overflow-hidden rounded-full bg-line"><div className="h-full rounded-full bg-accent transition-all" style={{ width: `${value * 100}%` }} /></div>;
}

function FleetBars({ byOs, total }: { byOs: Record<string, number>; total: number }) {
  const rows: { os: "windows" | "macos" | "linux"; label: string }[] = [{ os: "windows", label: "Windows" }, { os: "macos", label: "macOS" }, { os: "linux", label: "Linux" }];
  if (!total) return <p className="text-sm text-ink-3">No endpoints enrolled yet. <Link to="/enroll" className="text-accent hover:underline">Enroll one</Link>.</p>;
  return (
    <div className="space-y-3">
      {rows.map((r) => {
        const n = byOs[r.label] ?? 0;
        return (
          <div key={r.os} className="flex items-center gap-3 text-sm">
            <OsIcon os={r.os} /><span className="w-20 text-ink-2">{r.label}</span>
            <div className="h-2 flex-1 overflow-hidden rounded-full bg-panel-2"><div className="h-full rounded-full bg-accent" style={{ width: `${(n / total) * 100}%` }} /></div>
            <span className="w-8 text-right font-medium tabular-nums">{n}</span>
          </div>
        );
      })}
    </div>
  );
}

export function describe(action: string): string {
  const map: Record<string, string> = {
    "domain.setup": "created the domain", "auth.login": "signed in", "auth.failed": "failed to sign in as",
    "user.create": "created user", "user.update": "updated user", "user.delete": "deleted user", "user.password_reset": "reset the password of", "user.groups": "changed groups of",
    "group.create": "created group", "group.update": "updated group", "group.delete": "deleted group", "group.members": "changed members of",
    "ou.create": "created OU", "ou.update": "updated OU", "ou.delete": "deleted OU",
    "endpoint.enroll": "enrolled", "endpoint.update": "updated endpoint", "endpoint.retire": "retired endpoint",
    "job.create": "ran a script on", "job.cancel": "cancelled job",
    "policy.create": "created GPO", "policy.update": "updated GPO", "policy.delete": "deleted GPO",
    "enrollment_token.create": "created enrollment token", "enrollment_token.revoke": "revoked enrollment token",
  };
  return map[action] ?? action;
}
