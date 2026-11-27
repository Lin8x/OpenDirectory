// The console for delegated operators: accounts that aren't Domain Admins
// but have been delegated some endpoints, OUs or groups. They see only what
// was delegated to them, and only the actions their permissions allow.
import { useState, type ReactNode } from "react";
import { Link, NavLink, Navigate, Route, Routes, useParams } from "react-router-dom";
import { ArrowLeft, KeyRound, LifeBuoy, LogOut, Monitor, MonitorPlay, Moon, RefreshCw, RotateCw, Search, Sun, UserCog, UsersRound } from "lucide-react";
import { BrandMark } from "../lib/branding";
import { useQuery } from "../lib/hooks";
import { post } from "../lib/api";
import { useCan, useLearnMode, useSession } from "../lib/session";
import type { Endpoint, User } from "../lib/types";
import { ago, osLabel } from "../lib/format";
import { Avatar, Badge, Button, Card, Dialog, EmptyState, ErrorNote, Field, Input, Loading, OsIcon, PageHeader, StatusDot, Switch, cx, useToast } from "../components/ui";
import { RemoteViewer } from "../components/RemoteViewer";
import { RemoteShell } from "../components/RemoteShell";
import { ManageTab } from "../components/manage/Manage";
import { HelpDeskChat } from "../components/HelpDeskChat";
import { MfaCard } from "../components/Mfa";
import { LapsCard } from "./EndpointDetail";
import HelpDesk from "./HelpDesk";

export default function OperatorConsole() {
  const can = useCan();
  return (
    <OperatorShell>
      <Routes>
        <Route path="/" element={<MyDevices />} />
        <Route path="/endpoints" element={<Navigate to="/" replace />} />
        <Route path="/endpoints/:id" element={<OperatorDevice />} />
        {(can("view_users") || can("reset_passwords")) && <Route path="/users" element={<OperatorUsers />} />}
        {can("help_desk") && <Route path="/helpdesk" element={<HelpDesk />} />}
        <Route path="/account" element={<Account />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </OperatorShell>
  );
}

function OperatorShell({ children }: { children: ReactNode }) {
  const { me, signOut } = useSession();
  const can = useCan();
  const { theme, setTheme } = useLearnMode();
  const dark = theme === "dark" || (theme === "system" && document.documentElement.classList.contains("dark"));
  const items = [
    { to: "/", label: "My devices", icon: <Monitor className="size-4" /> },
    ...(can("view_users") || can("reset_passwords") ? [{ to: "/users", label: "Users", icon: <UsersRound className="size-4" /> }] : []),
    ...(can("help_desk") ? [{ to: "/helpdesk", label: "Help Desk", icon: <LifeBuoy className="size-4" /> }] : []),
    { to: "/account", label: "My account", icon: <UserCog className="size-4" /> },
  ];
  return (
    <div className="flex h-full">
      <aside className="flex w-56 shrink-0 flex-col border-r border-line bg-panel">
        <div className="flex items-center gap-2.5 px-4 pb-3 pt-4"><BrandMark sub={me?.domain} /></div>
        <nav className="flex-1 space-y-0.5 px-2">
          {items.map((i) => (
            <NavLink key={i.to} to={i.to} end={i.to === "/"} className={({ isActive }) => cx("flex h-8 items-center gap-2.5 rounded-md px-2.5 text-[13.5px]", isActive ? "bg-accent-soft font-medium text-accent" : "text-ink-2 hover:bg-panel-2 hover:text-ink")}>
              {i.icon}{i.label}
            </NavLink>
          ))}
        </nav>
        <div className="space-y-1 border-t border-line p-2">
          <button onClick={() => setTheme(dark ? "light" : "dark")} className="flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-[13px] text-ink-2 hover:bg-panel-2">{dark ? <Sun className="size-4" /> : <Moon className="size-4" />}{dark ? "Light theme" : "Dark theme"}</button>
          <div className="flex items-center gap-2.5 px-2 py-1.5">
            <Avatar name={me?.user.display_name || me?.user.username || "?"} className="size-7" />
            <div className="min-w-0 flex-1 leading-tight">
              <div className="truncate text-[13px] font-medium">{me?.user.display_name || me?.user.username}</div>
              <div className="truncate text-xs text-ink-3">Delegated operator</div>
            </div>
            <button title="Sign out" className="rounded p-1 text-ink-3 hover:bg-panel-2 hover:text-ink" onClick={() => void signOut()}><LogOut className="size-4" /></button>
          </div>
        </div>
      </aside>
      <main className="min-w-0 flex-1 overflow-y-auto"><div className="mx-auto max-w-[1280px] px-8 py-7">{children}</div></main>
    </div>
  );
}

function MyDevices() {
  const eps = useQuery<Endpoint[]>("/endpoints", { poll: 10000 });
  const can = useCan();
  const [q, setQ] = useState("");
  if (eps.loading && !eps.data) return <Loading />;
  if (eps.error && !eps.data) return <ErrorNote message={eps.error} onRetry={eps.reload} />;
  const list = (eps.data ?? []).filter((e) => !q || e.hostname.toLowerCase().includes(q.toLowerCase()));
  return (
    <div className="space-y-5">
      <PageHeader title="My devices" subtitle="The endpoints delegated to you. Open one to connect to it or manage it." />
      <div className="relative max-w-sm"><Search className="absolute left-2.5 top-2.5 size-4 text-ink-3" /><Input className="pl-8" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search devices" /></div>
      {list.length === 0 ? (
        <EmptyState icon={<Monitor className="size-6" />} title="No devices" body="Nothing has been delegated to you yet, or nothing matches the search. Ask a Domain Admin to delegate devices to your account." />
      ) : (
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
          {list.map((e) => (
            <Link key={e.id} to={`/endpoints/${e.id}`} className="group rounded-xl border border-line bg-panel p-4 transition hover:border-accent hover:shadow-md">
              <div className="flex items-center gap-3">
                <div className="grid size-10 place-items-center rounded-xl border border-line"><OsIcon os={e.os} className="size-5" /></div>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2 truncate font-medium">{e.hostname}<StatusDot online={e.online} /></div>
                  <div className="truncate text-xs text-ink-3">{e.inventory.os_name || osLabel[e.os]} · {e.online ? "Online" : `Seen ${ago(e.last_seen_at)}`}</div>
                </div>
              </div>
              {can("remote_desktop") && e.os !== "android" && <div className="mt-3 flex items-center gap-1.5 text-sm text-accent"><MonitorPlay className="size-4" />Connect</div>}
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}

function OperatorDevice() {
  const { id } = useParams();
  const ep = useQuery<Endpoint>(`/endpoints/${id}`, { poll: 8000 });
  const can = useCan();
  const toast = useToast();
  if (ep.loading && !ep.data) return <Loading />;
  if (ep.error && !ep.data) return <ErrorNote message={ep.error} onRetry={ep.reload} />;
  const e = ep.data!;
  const manageTools = ([["services", "manage_services"], ["software", "manage_software"], ["processes", "manage_processes"], ["containers", "manage_containers"]] as const)
    .filter(([, p]) => can(p)).map(([t]) => t);
  const job = async (type: string, msg: string) => {
    try { await post(`/endpoints/${e.id}/jobs`, { type }); toast("good", msg); } catch (err) { toast("bad", (err as Error).message); }
  };
  return (
    <div className="space-y-5">
      <Link to="/" className="inline-flex items-center gap-1 text-sm text-ink-3 hover:text-ink"><ArrowLeft className="size-4" />My devices</Link>
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div className="flex items-center gap-4">
          <div className="grid size-14 place-items-center rounded-2xl border border-line bg-panel"><OsIcon os={e.os} className="size-7" /></div>
          <div>
            <h1 className="flex items-center gap-2.5 text-[22px] font-semibold tracking-tight">{e.hostname}<StatusDot online={e.online} /></h1>
            <p className="text-sm text-ink-2">{e.inventory.os_name} · {e.online ? <span className="text-good">Online</span> : `Last seen ${ago(e.last_seen_at)}`}</p>
          </div>
        </div>
        {can("endpoint_actions") && (
          <div className="flex gap-2">
            <Button icon={<RefreshCw className="size-4" />} onClick={() => job("refresh_inventory", "Inventory refresh requested")}>Refresh inventory</Button>
            {e.os !== "android" && <Button icon={<RotateCw className="size-4" />} onClick={() => job("restart_agent", "Agent restart requested")}>Restart agent</Button>}
          </div>
        )}
      </div>
      {can("remote_desktop") && <RemoteViewer endpoint={e} />}
      {can("read_laps") && <LapsCard id={e.id} />}
      {can("remote_shell") && e.os !== "android" && <RemoteShell endpoint={e} />}
      {e.os !== "android" && manageTools.length > 0 && <Card title="Manage" padded={false}><div className="p-4"><ManageTab endpoint={e} tools={manageTools} /></div></Card>}
      {can("help_desk") && <Card title="Help desk" padded={false}><HelpDeskChat endpointId={e.id} hostname={e.hostname} /></Card>}
      <Card title="About this device">
        <div className="grid gap-3 text-sm sm:grid-cols-3">
          <div><div className="text-xs text-ink-3">Model</div>{[e.inventory.manufacturer, e.inventory.model].filter(Boolean).join(" ") || "—"}</div>
          <div><div className="text-xs text-ink-3">Signed in</div>{e.inventory.logged_in_users?.join(", ") || "Nobody"}</div>
          <div><div className="text-xs text-ink-3">Agent</div>{e.agent_version}</div>
        </div>
      </Card>
    </div>
  );
}

function OperatorUsers() {
  const users = useQuery<User[]>("/users");
  const can = useCan();
  const toast = useToast();
  const [reset, setReset] = useState<User | null>(null);
  const [pw, setPw] = useState("");
  const [must, setMust] = useState(true);
  if (users.loading && !users.data) return <Loading />;
  if (users.error && !users.data) return <ErrorNote message={users.error} onRetry={users.reload} />;
  return (
    <div className="space-y-5">
      <PageHeader title="Users" subtitle="Accounts in the OUs and groups delegated to you." />
      <Card padded={false}>
        {(users.data ?? []).length === 0 ? <div className="p-6 text-sm text-ink-3">No accounts are in your delegated scope.</div> : (
          <div className="divide-y divide-line">
            {users.data!.map((u) => (
              <div key={u.id} className="flex items-center gap-3 px-4 py-3">
                <Avatar name={u.display_name || u.username} className="size-8" />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-medium">{u.display_name || u.username}</div>
                  <div className="truncate text-xs text-ink-3">{u.username}{u.email ? ` · ${u.email}` : ""}</div>
                </div>
                {!u.enabled && <Badge tone="warn">Disabled</Badge>}
                {can("reset_passwords") && <Button size="sm" icon={<KeyRound className="size-3.5" />} onClick={() => { setReset(u); setPw(""); setMust(true); }}>Reset password</Button>}
              </div>
            ))}
          </div>
        )}
      </Card>
      <Dialog open={!!reset} onClose={() => setReset(null)} title={`Reset password for ${reset?.username}`}
        footer={<><Button onClick={() => setReset(null)}>Cancel</Button><Button variant="primary" disabled={pw.length < 8} onClick={async () => {
          try { await post(`/users/${reset!.id}/password`, { password: pw, must_change_password: must }); toast("good", "Password reset"); setReset(null); } catch (e) { toast("bad", (e as Error).message); }
        }}>Reset</Button></>}>
        <div className="space-y-3">
          <Field label="New password"><Input type="password" value={pw} onChange={(e) => setPw(e.target.value)} autoComplete="new-password" /></Field>
          <Switch checked={must} onChange={setMust} label="Must change it at next sign-in" />
        </div>
      </Dialog>
    </div>
  );
}

function Account() {
  const { me } = useSession();
  return (
    <div className="space-y-5">
      <PageHeader title="My account" subtitle={`Signed in as ${me?.user.username}`} />
      <MfaCard />
      <Card title="What's delegated to you" subtitle="Ask a Domain Admin if you need more, or less.">
        <div className="space-y-2">
          {me?.delegations.map((d) => (
            <div key={d.id} className="flex flex-wrap items-center gap-2 text-sm">
              <b>{d.name}</b>
              {d.permissions.map((p) => <Badge key={p}>{PERMISSION_LABELS[p] ?? p}</Badge>)}
              {d.expires_at && <span className="text-xs text-ink-3">until {new Date(d.expires_at).toLocaleString()}</span>}
            </div>
          ))}
        </div>
      </Card>
    </div>
  );
}

export const PERMISSION_LABELS: Record<string, string> = {
  view_endpoints: "View endpoints",
  remote_desktop: "Remote desktop",
  remote_shell: "Remote shell",
  endpoint_actions: "Endpoint actions",
  run_scripts: "Run scripts",
  read_laps: "Read LAPS password",
  help_desk: "Help desk chat",
  view_users: "View users",
  reset_passwords: "Reset passwords",
  manage_services: "Manage services",
  manage_software: "Manage software",
  manage_processes: "End processes",
  manage_containers: "Manage containers",
};
