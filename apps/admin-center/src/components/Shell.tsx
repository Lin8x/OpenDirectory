import { useEffect, useRef, useState, type ReactNode } from "react";
import { NavLink, useLocation, useNavigate } from "react-router-dom";
import {
  Activity, ArrowRight, Download, BookOpen, Boxes, DatabaseBackup, FlaskConical, FolderTree, GraduationCap, LayoutDashboard, LogOut, Monitor, Moon, Network, PlusCircle, ScrollText,
  KeyRound, PanelsTopLeft, Plug, Trash2, Search, Server, Settings as SettingsIcon, ShieldCheck, Sun, Terminal, Users as UsersIcon, UsersRound,
} from "lucide-react";
import { useLearnMode, useSession } from "../lib/session";
import { get } from "../lib/api";
import type { SearchHit } from "../lib/types";
import { Avatar, Switch, cx } from "./ui";
import { BrandMark } from "../lib/branding";

const NAV: { section: string; items: { to: string; label: string; icon: ReactNode }[] }[] = [
  { section: "", items: [{ to: "/", label: "Overview", icon: <LayoutDashboard className="size-4" /> }] },
  {
    section: "Directory",
    items: [
      { to: "/users", label: "Users", icon: <UsersIcon className="size-4" /> },
      { to: "/groups", label: "Security Groups", icon: <UsersRound className="size-4" /> },
      { to: "/ous", label: "Organizational Units", icon: <FolderTree className="size-4" /> },
      { to: "/password-policies", label: "Password Policies", icon: <KeyRound className="size-4" /> },
      { to: "/recycle-bin", label: "Recycle Bin", icon: <Trash2 className="size-4" /> },
    ],
  },
  {
    section: "Endpoints",
    items: [
      { to: "/endpoints", label: "Endpoints", icon: <Monitor className="size-4" /> },
      { to: "/enroll", label: "Enrollment", icon: <PlusCircle className="size-4" /> },
      { to: "/scripts", label: "Script Library", icon: <Terminal className="size-4" /> },
      { to: "/patches", label: "Patch Management", icon: <Download className="size-4" /> },
    ],
  },
  { section: "Policy", items: [{ to: "/gpos", label: "Group Policy", icon: <ShieldCheck className="size-4" /> }] },
  {
    section: "Monitoring",
    items: [
      { to: "/topology", label: "Topology Map", icon: <Network className="size-4" /> },
      { to: "/audit", label: "Audit Log", icon: <ScrollText className="size-4" /> },
    ],
  },
  {
    section: "Integrations",
    items: [
      { to: "/integrations?tab=windows", label: "Windows Services", icon: <Server className="size-4" /> },
      { to: "/integrations?tab=cloud", label: "Cloud & Alerts", icon: <Plug className="size-4" /> },
    ],
  },
  {
    section: "Administration",
    items: [
      { to: "/lab", label: "Network Lab", icon: <FlaskConical className="size-4" /> },
      { to: "/backups", label: "Backup & Restore", icon: <DatabaseBackup className="size-4" /> },
    ],
  },
];

export default function Shell({ children }: { children: ReactNode }) {
  const { me, signOut } = useSession();
  const { learn, setLearn, theme, setTheme, setUi } = useLearnMode();
  const [palette, setPalette] = useState(false);
  const loc = useLocation();
  const [menu, setMenu] = useState(false);
  const dark = theme === "dark" || (theme === "system" && document.documentElement.classList.contains("dark"));

  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") { e.preventDefault(); setPalette(true); }
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, []);

  return (
    <div className="flex h-full">
      <aside className="flex w-60 shrink-0 flex-col border-r border-line bg-panel">
        <div className="flex items-center gap-2.5 px-4 pb-3 pt-4">
          <BrandMark sub={me?.domain} />
        </div>
        <button onClick={() => setPalette(true)} className="mx-3 mb-2 flex h-8 items-center gap-2 rounded-md border border-line bg-panel-2 px-2.5 text-[13px] text-ink-3 hover:text-ink">
          <Search className="size-3.5" /><span className="flex-1 text-left">Search</span><kbd className="rounded border border-line bg-panel px-1 text-[10px]">Ctrl K</kbd>
        </button>
        <nav className="flex-1 overflow-y-auto px-2 pb-3">
          {NAV.map((g) => (
            <div key={g.section} className="mt-3 first:mt-1">
              {g.section && <div className="px-2.5 pb-1 text-[11px] font-semibold uppercase tracking-wider text-ink-3">{g.section}</div>}
              {g.items.map((i) => (
                <NavLink key={i.to} to={i.to} end={i.to === "/"}
                  className={({ isActive }) => cx("flex h-8 items-center gap-2.5 rounded-md px-2.5 text-[13.5px] transition",
                    isActive && (!i.to.includes("?") || loc.search === i.to.slice(i.to.indexOf("?")) || (!loc.search && i.to.endsWith("=windows"))) ? "bg-accent-soft font-medium text-accent" : "text-ink-2 hover:bg-panel-2 hover:text-ink")}>
                  {i.icon}{i.label}
                </NavLink>
              ))}
            </div>
          ))}
        </nav>
        <div className="space-y-1 border-t border-line p-2">
          <NavLink to="/learn" className={({ isActive }) => cx("flex h-8 items-center gap-2.5 rounded-md px-2.5 text-[13.5px]", isActive ? "bg-accent-soft text-accent font-medium" : "text-ink-2 hover:bg-panel-2")}>
            <BookOpen className="size-4" />Learn
          </NavLink>
          <div className="flex h-8 items-center justify-between rounded-md px-2.5 text-[13.5px] text-ink-2">
            <span className="flex items-center gap-2.5"><GraduationCap className="size-4" />Learn mode</span>
            <Switch checked={learn} onChange={setLearn} />
          </div>
          <div className="relative">
            <button onClick={() => setMenu((m) => !m)} className="flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left hover:bg-panel-2">
              <Avatar name={me?.user.display_name ?? "?"} className="size-7" />
              <div className="min-w-0 flex-1 leading-tight">
                <div className="truncate text-[13px] font-medium">{me?.user.display_name}</div>
                <div className="truncate text-xs text-ink-3">Domain Admin</div>
              </div>
            </button>
            {menu && (
              <div className="anim-pop absolute bottom-11 left-0 right-0 z-30 rounded-lg border border-line bg-panel p-1 shadow-xl" onMouseLeave={() => setMenu(false)}>
                <MenuItem icon={dark ? <Sun className="size-4" /> : <Moon className="size-4" />} onClick={() => { setTheme(dark ? "light" : "dark"); setMenu(false); }}>{dark ? "Light theme" : "Dark theme"}</MenuItem>
                <NavLink to="/settings" onClick={() => setMenu(false)} className="flex h-8 items-center gap-2.5 rounded-md px-2.5 text-[13px] text-ink-2 hover:bg-panel-2"><SettingsIcon className="size-4" />Server &amp; connection</NavLink>
                <button onClick={() => { setMenu(false); setUi("classic"); }} className="flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-[13px] text-ink-2 hover:bg-panel-2"><PanelsTopLeft className="size-4" />Classic interface</button>
                <MenuItem icon={<LogOut className="size-4" />} onClick={() => void signOut()}>Sign out</MenuItem>
              </div>
            )}
          </div>
        </div>
      </aside>
      <main className="min-w-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[1280px] px-8 py-7">{children}</div>
      </main>
      {palette && <CommandPalette onClose={() => setPalette(false)} />}
    </div>
  );
}

function MenuItem({ icon, children, onClick }: { icon: ReactNode; children: ReactNode; onClick: () => void }) {
  return <button onClick={onClick} className="flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-left text-[13px] text-ink-2 hover:bg-panel-2">{icon}{children}</button>;
}

const HIT_ICON: Record<SearchHit["type"], ReactNode> = {
  user: <UsersIcon className="size-4" />, group: <UsersRound className="size-4" />, endpoint: <Monitor className="size-4" />,
  ou: <FolderTree className="size-4" />, policy: <ShieldCheck className="size-4" />,
};
const QUICK = [
  { label: "Enroll an endpoint", to: "/enroll", icon: <PlusCircle className="size-4" /> },
  { label: "Create a user", to: "/users?new=1", icon: <UsersIcon className="size-4" /> },
  { label: "Create a Group Policy Object", to: "/gpos/new", icon: <ShieldCheck className="size-4" /> },
  { label: "Run a script from the library", to: "/scripts", icon: <Terminal className="size-4" /> },
  { label: "View audit log", to: "/audit", icon: <Activity className="size-4" /> },
  { label: "Learn the concepts", to: "/learn", icon: <Boxes className="size-4" /> },
];

const PAGES = [
  { label: "Overview", to: "/" }, { label: "Users", to: "/users" }, { label: "Security Groups", to: "/groups" },
  { label: "Organizational Units", to: "/ous" }, { label: "Endpoints", to: "/endpoints" }, { label: "Enrollment", to: "/enroll" },
  { label: "Script Library", to: "/scripts" }, { label: "Patch Management", to: "/patches" }, { label: "Group Policy", to: "/gpos" }, { label: "Audit Log", to: "/audit" }, { label: "Backup & Restore", to: "/backups" }, { label: "Recycle Bin", to: "/recycle-bin" }, { label: "Password Policies", to: "/password-policies" }, { label: "Network Lab", to: "/lab" }, { label: "Topology Map", to: "/topology" }, { label: "Windows Services (integrations)", to: "/integrations?tab=windows" }, { label: "Cloud & Alerts (integrations)", to: "/integrations?tab=cloud" },
  { label: "Learn", to: "/learn" }, { label: "Server & connection settings", to: "/settings" },
];

function CommandPalette({ onClose }: { onClose: () => void }) {
  const [q, setQ] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [sel, setSel] = useState(0);
  const nav = useNavigate();
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => { input.current?.focus(); }, []);
  useEffect(() => {
    if (!q.trim()) { setHits([]); return; }
    const t = setTimeout(() => { get<SearchHit[]>(`/search?q=${encodeURIComponent(q)}`).then(setHits).catch(() => setHits([])); }, 120);
    return () => clearTimeout(t);
  }, [q]);

  const needle = q.trim().toLowerCase();
  const items = needle
    ? [
        ...QUICK.filter((x) => x.label.toLowerCase().includes(needle)).map((x) => ({ ...x, sub: "Action" })),
        ...PAGES.filter((x) => x.label.toLowerCase().includes(needle)).map((x) => ({ ...x, icon: <ArrowRight className="size-4" />, label: `Go to ${x.label}`, sub: "Page" })),
        ...hits.map((h) => ({ label: h.title, sub: h.subtitle, icon: HIT_ICON[h.type], to: hitPath(h) })),
      ]
    : QUICK.map((x) => ({ ...x, sub: "" }));

  const go = (to: string) => { nav(to); onClose(); };
  return (
    <div className="anim-fade fixed inset-0 z-50 bg-black/40 pt-[12vh] backdrop-blur-[2px]" onMouseDown={onClose}>
      <div className="anim-pop mx-auto w-full max-w-xl overflow-hidden rounded-xl border border-line bg-panel shadow-2xl" onMouseDown={(e) => e.stopPropagation()}>
        <div className="flex items-center gap-2.5 border-b border-line px-4">
          <Search className="size-4 text-ink-3" />
          <input ref={input} value={q} onChange={(e) => { setQ(e.target.value); setSel(0); }} placeholder="Search or jump to… users, endpoints (hostname, serial, IP), groups, GPOs, pages"
            onKeyDown={(e) => {
              if (e.key === "Escape") onClose();
              if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => Math.min(items.length - 1, s + 1)); }
              if (e.key === "Tab") e.preventDefault();
              if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => Math.max(0, s - 1)); }
              if (e.key === "Enter" && items[sel]) go(items[sel].to);
            }}
            className="h-12 flex-1 bg-transparent text-[15px] outline-none placeholder:text-ink-3" />
        </div>
        <div className="max-h-80 overflow-y-auto p-1.5">
          {!q.trim() && <div className="px-2.5 pb-1 pt-1.5 text-[11px] font-semibold uppercase tracking-wider text-ink-3">Quick actions</div>}
          {items.length === 0 && <div className="px-3 py-6 text-center text-sm text-ink-3">No matches</div>}
          {items.map((it, i) => (
            <button key={i} onMouseEnter={() => setSel(i)} onClick={() => go(it.to)}
              className={cx("flex w-full items-center gap-3 rounded-md px-2.5 py-2 text-left text-sm", i === sel ? "bg-accent-soft text-ink" : "text-ink-2")}>
              <span className="text-ink-3">{it.icon}</span><span className="flex-1 truncate">{it.label}</span>
              {it.sub && <span className="truncate text-xs text-ink-3">{it.sub}</span>}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

function hitPath(h: SearchHit) {
  switch (h.type) {
    case "user": return `/users?id=${h.id}`;
    case "group": return `/groups?id=${h.id}`;
    case "endpoint": return `/endpoints/${h.id}`;
    case "ou": return `/ous?id=${h.id}`;
    case "policy": return `/gpos/${h.id}`;
  }
}
