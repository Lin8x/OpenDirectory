// Classic interface: an MMC-style console for admins coming from Windows
// Server. A console tree on the left, a details list on the right, a menu
// bar, right-click menus and tabbed property sheets. It mimics the layout and
// workflow of Active Directory Users and Computers and Group Policy
// Management, with OpenDirectory's own icons. Pages without a classic view
// open in the right-hand pane.
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import {
  ChevronDown, ChevronRight, DatabaseBackup, FileText, Folder, FolderOpen, Globe, KeyRound, LayoutGrid, Monitor, Plug, RefreshCw,
  ScrollText, Settings, ShieldCheck, Terminal, User as UserIcon, UserX, UsersRound,
} from "lucide-react";
import { useQuery } from "../lib/hooks";
import { del, patch, post, put } from "../lib/api";
import { useLearnMode, useSession } from "../lib/session";
import type { Endpoint, Group, OrgUnit, Policy, User } from "../lib/types";
import { Button, Dialog, Field, Input, Switch, Tabs, Textarea, cx, useToast } from "./ui";
import { BrandMark } from "../lib/branding";

type Node =
  | { kind: "root" } | { kind: "aduc" } | { kind: "domain" } | { kind: "ou"; id: string } | { kind: "users" } | { kind: "computers" }
  | { kind: "gpmc" } | { kind: "gpmc-domain" } | { kind: "gpmc-ou"; id: string } | { kind: "gpos" } | { kind: "page"; to: string };
type Item = { type: "ou" | "user" | "group" | "computer" | "gpo" | "link"; id: string; name: string; typeLabel: string; description: string; icon: ReactNode; disabled?: boolean };

const key = (n: Node) => JSON.stringify(n);

export default function ClassicShell({ children }: { children: ReactNode }) {
  const { me, signOut } = useSession();
  const { setUi } = useLearnMode();
  const nav = useNavigate();
  const loc = useLocation();
  const ous = useQuery<OrgUnit[]>("/ous");
  const users = useQuery<User[]>("/users");
  const groups = useQuery<Group[]>("/groups");
  const eps = useQuery<Endpoint[]>("/endpoints");
  const gpos = useQuery<Policy[]>("/policies");
  const [sel, setSel] = useState<Node>({ kind: "domain" });
  const [open, setOpen] = useState<Set<string>>(new Set([key({ kind: "root" }), key({ kind: "aduc" }), key({ kind: "domain" }), key({ kind: "gpmc" }), key({ kind: "gpmc-domain" })]));
  const [picked, setPicked] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; item: Item | null } | null>(null);
  const [dialog, setDialog] = useState<null | { kind: "user-props"; user: User } | { kind: "group-props"; group: Group } | { kind: "new-user" | "new-group" | "new-ou" } | { kind: "reset"; user: User } | { kind: "delete"; item: Item }>(null);
  const [menubar, setMenubar] = useState<string | null>(null);
  const reload = () => { void ous.reload(); void users.reload(); void groups.reload(); void eps.reload(); void gpos.reload(); };
  const domain = me?.domain ?? "domain";

  // Leaving a classic node for a page shows that page in the right pane.
  useEffect(() => { if (loc.pathname !== "/" && sel.kind !== "page") setSel({ kind: "page", to: loc.pathname }); }, [loc.pathname]); // eslint-disable-line react-hooks/exhaustive-deps
  const go = (n: Node) => { setSel(n); setPicked(null); if (n.kind === "page") nav(n.to); else if (loc.pathname !== "/") nav("/"); };
  const toggle = (n: Node) => setOpen((o) => { const s = new Set(o); const k = key(n); if (s.has(k)) s.delete(k); else s.add(k); return s; });

  const ouList = ous.data ?? [];
  const childOus = (parent: string | null) => ouList.filter((o) => o.parent_id === parent).sort((a, b) => a.name.localeCompare(b.name));
  const linkedTo = (ou: string | null) => (gpos.data ?? []).filter((p) => (ou === null ? p.targets.all_endpoints : p.targets.ou_ids.includes(ou)));
  const currentOu = sel.kind === "ou" ? sel.id : sel.kind === "gpmc-ou" ? sel.id : null;

  const items: Item[] = useMemo(() => {
    const u = (x: User): Item => ({ type: "user", id: x.id, name: x.display_name, typeLabel: "User", description: x.title || x.username, icon: x.enabled ? <UserIcon className="size-3.5 text-sky-600" /> : <UserX className="size-3.5 text-ink-3" />, disabled: !x.enabled });
    const g = (x: Group): Item => ({ type: "group", id: x.id, name: x.name, typeLabel: "Security Group", description: x.description, icon: <UsersRound className="size-3.5 text-amber-600" /> });
    const c = (x: Endpoint): Item => ({ type: "computer", id: x.id, name: x.hostname, typeLabel: "Computer", description: `${x.inventory.os_name || x.os} · ${x.online ? "online" : "offline"}`, icon: <Monitor className="size-3.5 text-emerald-600" /> });
    const o = (x: OrgUnit): Item => ({ type: "ou", id: x.id, name: x.name, typeLabel: "Organizational Unit", description: x.description, icon: <Folder className="size-3.5 text-yellow-600" /> });
    const p = (x: Policy, link = false): Item => ({ type: link ? "link" : "gpo", id: x.id, name: x.name, typeLabel: link ? "GPO link" : "Group Policy Object", description: `${x.enabled ? "Enabled" : "Disabled"} · priority ${x.priority} · ${x.settings.length} settings`, icon: <ScrollText className="size-3.5 text-violet-600" />, disabled: !x.enabled });
    switch (sel.kind) {
      case "domain": return [...childOus(null).map(o), { type: "ou", id: "__users", name: "Users", typeLabel: "Container", description: "Users and groups not in an OU", icon: <Folder className="size-3.5 text-yellow-600" /> }, { type: "ou", id: "__computers", name: "Computers", typeLabel: "Container", description: "Computers not in an OU", icon: <Folder className="size-3.5 text-yellow-600" /> }];
      case "users": return [...(users.data ?? []).filter((x) => !x.ou_id).map(u), ...(groups.data ?? []).filter((x) => !x.ou_id).map(g)];
      case "computers": return (eps.data ?? []).filter((x) => !x.ou_id).map(c);
      case "ou": return [...childOus(sel.id).map(o), ...(users.data ?? []).filter((x) => x.ou_id === sel.id).map(u), ...(groups.data ?? []).filter((x) => x.ou_id === sel.id).map(g), ...(eps.data ?? []).filter((x) => x.ou_id === sel.id).map(c)];
      case "gpmc-domain": return [...linkedTo(null).map((x) => p(x, true)), ...childOus(null).map(o)];
      case "gpmc-ou": return [...linkedTo(sel.id).map((x) => p(x, true)), ...childOus(sel.id).map(o)];
      case "gpos": return (gpos.data ?? []).map((x) => p(x));
      default: return [];
    }
  }, [sel, ous.data, users.data, groups.data, eps.data, gpos.data]); // eslint-disable-line react-hooks/exhaustive-deps

  const activate = (it: Item) => {
    if (it.type === "ou") {
      if (it.id === "__users") return go({ kind: "users" });
      if (it.id === "__computers") return go({ kind: "computers" });
      setOpen((o) => new Set(o).add(key(sel)));
      return go(sel.kind.startsWith("gpmc") ? { kind: "gpmc-ou", id: it.id } : { kind: "ou", id: it.id });
    }
    if (it.type === "user") { const x = users.data?.find((u) => u.id === it.id); if (x) setDialog({ kind: "user-props", user: x }); }
    if (it.type === "group") { const x = groups.data?.find((g) => g.id === it.id); if (x) setDialog({ kind: "group-props", group: x }); }
    if (it.type === "computer") go({ kind: "page", to: `/endpoints/${it.id}` });
    if (it.type === "gpo" || it.type === "link") go({ kind: "page", to: `/gpos/${it.id}` });
  };
  const pickedItem = items.find((i) => i.id === picked) ?? null;
  const canCreate = sel.kind === "ou" || sel.kind === "domain" || sel.kind === "users";

  const actions = (it: Item | null): { label: string; run: () => void; sep?: boolean; disabled?: boolean }[] => {
    const a: { label: string; run: () => void; sep?: boolean; disabled?: boolean }[] = [];
    if (canCreate) a.push({ label: "New User…", run: () => setDialog({ kind: "new-user" }) }, { label: "New Group…", run: () => setDialog({ kind: "new-group" }) }, { label: "New Organizational Unit…", run: () => setDialog({ kind: "new-ou" }) });
    if (sel.kind === "gpos" || sel.kind.startsWith("gpmc")) a.push({ label: "New GPO…", run: () => go({ kind: "page", to: "/gpos/new" }) });
    if (it) {
      a.push({ label: "", run: () => {}, sep: true });
      if (it.type === "user") {
        const u = users.data?.find((x) => x.id === it.id)!;
        a.push({ label: "Reset Password…", run: () => setDialog({ kind: "reset", user: u }) });
        a.push({ label: u.enabled ? "Disable Account" : "Enable Account", run: async () => { await patch(`/users/${u.id}`, { enabled: !u.enabled }).catch((e) => toast("bad", (e as Error).message)); reload(); } });
      }
      if (it.type === "computer") a.push({ label: "Run Script…", run: () => go({ kind: "page", to: "/scripts" }) }, { label: "Resultant Set of Policy", run: () => go({ kind: "page", to: `/endpoints/${it.id}` }) });
      if (it.type === "link" || it.type === "gpo") a.push({ label: "Edit…", run: () => go({ kind: "page", to: `/gpos/${it.id}` }) });
      if (it.type !== "link" && !it.id.startsWith("__")) a.push({ label: "Delete", run: () => setDialog({ kind: "delete", item: it }) });
      a.push({ label: "Properties", run: () => activate(it) });
    }
    a.push({ label: "", run: () => {}, sep: true }, { label: "Refresh", run: reload });
    return a.filter((x, i, arr) => !(x.sep && (i === 0 || arr[i - 1]!.sep)));
  };
  const toast = useToast();

  const T = ({ n, label, icon, depth, kids }: { n: Node; label: string; icon: ReactNode; depth: number; kids?: ReactNode }) => {
    const k = key(n);
    const isOpen = open.has(k);
    const active = key(sel) === k || (n.kind === "page" && sel.kind === "page" && loc.pathname === n.to);
    return (
      <>
        <div className={cx("flex h-[22px] cursor-default select-none items-center gap-1 whitespace-nowrap pr-2", active ? "bg-[#cce8ff] text-black dark:bg-[#264f78] dark:text-white" : "hover:bg-[#e5f3ff] dark:hover:bg-white/5")}
          style={{ paddingLeft: depth * 14 + 4 }} onClick={() => go(n)} onDoubleClick={() => toggle(n)}>
          <span className="grid size-3.5 place-items-center" onClick={(e) => { e.stopPropagation(); toggle(n); }}>{kids ? (isOpen ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />) : null}</span>
          {icon}<span>{label}</span>
        </div>
        {kids && isOpen && kids}
      </>
    );
  };
  const ouTree = (parent: string | null, depth: number, gp: boolean): ReactNode =>
    childOus(parent).map((o) => {
      const n: Node = gp ? { kind: "gpmc-ou", id: o.id } : { kind: "ou", id: o.id };
      const has = childOus(o.id).length > 0;
      return <T key={o.id} n={n} depth={depth} label={o.name} icon={open.has(key(n)) ? <FolderOpen className="size-3.5 text-yellow-600" /> : <Folder className="size-3.5 text-yellow-600" />} kids={has ? ouTree(o.id, depth + 1, gp) : undefined} />;
    });

  const title = sel.kind === "page" ? "" : sel.kind === "domain" || sel.kind === "gpmc-domain" ? domain : sel.kind === "users" ? "Users" : sel.kind === "computers" ? "Computers" : sel.kind === "gpos" ? "Group Policy Objects" : currentOu ? ouList.find((o) => o.id === currentOu)?.name ?? "" : "";
  const MENUS: Record<string, { label: string; run: () => void; sep?: boolean }[]> = {
    File: [{ label: "Switch to the modern interface", run: () => { setUi("modern"); } }, { label: "", run: () => {}, sep: true }, { label: "Sign out", run: signOut }],
    Action: actions(pickedItem),
    View: [{ label: "Refresh", run: reload }, { label: "Modern interface", run: () => setUi("modern") }],
    Help: [{ label: "Learn the concepts", run: () => go({ kind: "page", to: "/learn" }) }, { label: "About OpenDirectory", run: () => go({ kind: "page", to: "/settings" }) }],
  };

  return (
    <div className="flex h-full flex-col bg-[#f0f0f0] text-[12px] text-[#1b1b1b] dark:bg-[#1f1f1f] dark:text-[#e6e6e6]" onClick={() => { setMenu(null); setMenubar(null); }}>
      <div className="flex h-7 items-center gap-2 border-b border-[#d9d9d9] bg-white px-2 dark:border-white/10 dark:bg-[#2b2b2b]">
        <div className="flex items-center gap-1.5 [&_img]:size-4 [&_div.leading-tight]:hidden"><BrandMark /></div>
        <span className="font-medium">{me?.organization ?? "OpenDirectory"} Admin Center</span>
      </div>
      <div className="relative flex h-6 items-center border-b border-[#d9d9d9] bg-[#f8f8f8] px-1 dark:border-white/10 dark:bg-[#252525]">
        {Object.keys(MENUS).map((m) => (
          <div key={m} className="relative">
            <button className={cx("px-2 py-0.5", menubar === m && "bg-[#cce8ff] dark:bg-[#264f78]")} onClick={(e) => { e.stopPropagation(); setMenubar(menubar === m ? null : m); }}>{m}</button>
            {menubar === m && <MenuList items={MENUS[m]!} x={0} y={22} onDone={() => setMenubar(null)} />}
          </div>
        ))}
      </div>
      <div className="flex h-8 items-center gap-1 border-b border-[#d9d9d9] bg-[#f8f8f8] px-2 dark:border-white/10 dark:bg-[#252525]">
        <ToolBtn title="Refresh" onClick={reload}><RefreshCw className="size-3.5" /></ToolBtn>
        <span className="mx-1 h-4 w-px bg-[#ccc]" />
        <ToolBtn title="New User" disabled={!canCreate} onClick={() => setDialog({ kind: "new-user" })}><UserIcon className="size-3.5" /></ToolBtn>
        <ToolBtn title="New Group" disabled={!canCreate} onClick={() => setDialog({ kind: "new-group" })}><UsersRound className="size-3.5" /></ToolBtn>
        <ToolBtn title="New Organizational Unit" disabled={!canCreate} onClick={() => setDialog({ kind: "new-ou" })}><Folder className="size-3.5" /></ToolBtn>
        <ToolBtn title="Properties" disabled={!pickedItem} onClick={() => pickedItem && activate(pickedItem)}><FileText className="size-3.5" /></ToolBtn>
        <ToolBtn title="Reset Password" disabled={pickedItem?.type !== "user"} onClick={() => { const u = users.data?.find((x) => x.id === picked); if (u) setDialog({ kind: "reset", user: u }); }}><KeyRound className="size-3.5" /></ToolBtn>
        <div className="flex-1" />
        <button className="rounded border border-[#ccc] bg-white px-2 py-0.5 hover:bg-[#e5f3ff] dark:border-white/15 dark:bg-[#333]" onClick={() => setUi("modern")}>Modern interface</button>
      </div>
      <div className="flex min-h-0 flex-1">
        <div className="w-64 shrink-0 overflow-auto border-r border-[#d9d9d9] bg-white py-1 dark:border-white/10 dark:bg-[#252526]">
          <T n={{ kind: "root" }} depth={0} label="Admin Center" icon={<LayoutGrid className="size-3.5" />} kids={<>
            <T n={{ kind: "aduc" }} depth={1} label="Users and Computers" icon={<UsersRound className="size-3.5 text-sky-600" />} kids={
              <T n={{ kind: "domain" }} depth={2} label={domain} icon={<Globe className="size-3.5 text-sky-700" />} kids={<>
                {ouTree(null, 3, false)}
                <T n={{ kind: "users" }} depth={3} label="Users" icon={<Folder className="size-3.5 text-yellow-600" />} />
                <T n={{ kind: "computers" }} depth={3} label="Computers" icon={<Folder className="size-3.5 text-yellow-600" />} />
              </>} />
            } />
            <T n={{ kind: "gpmc" }} depth={1} label="Group Policy Management" icon={<ShieldCheck className="size-3.5 text-violet-600" />} kids={
              <T n={{ kind: "gpmc-domain" }} depth={2} label={domain} icon={<Globe className="size-3.5 text-sky-700" />} kids={<>
                {ouTree(null, 3, true)}
                <T n={{ kind: "gpos" }} depth={3} label="Group Policy Objects" icon={<Folder className="size-3.5 text-yellow-600" />} />
              </>} />
            } />
            <T n={{ kind: "page", to: "/endpoints" }} depth={1} label="Endpoints" icon={<Monitor className="size-3.5 text-emerald-600" />} />
            <T n={{ kind: "page", to: "/scripts" }} depth={1} label="Script Library" icon={<Terminal className="size-3.5" />} />
            <T n={{ kind: "page", to: "/audit" }} depth={1} label="Event Viewer (Audit Log)" icon={<ScrollText className="size-3.5" />} />
            <T n={{ kind: "page", to: "/backups" }} depth={1} label="Backup & Restore" icon={<DatabaseBackup className="size-3.5" />} />
            <T n={{ kind: "page", to: "/integrations" }} depth={1} label="Integrations" icon={<Plug className="size-3.5" />} />
            <T n={{ kind: "page", to: "/settings" }} depth={1} label="Settings" icon={<Settings className="size-3.5" />} />
          </>} />
        </div>
        <div className="min-w-0 flex-1 overflow-auto bg-white dark:bg-[#1e1e1e]">
          {sel.kind === "page" ? <div className="p-5 text-sm">{children}</div> : ["root", "aduc", "gpmc"].includes(sel.kind) ? (
            <div className="p-6 text-[13px] leading-relaxed text-ink-2">
              <h2 className="mb-2 text-base font-semibold text-ink">{sel.kind === "gpmc" ? "Group Policy Management" : sel.kind === "aduc" ? "Users and Computers" : "Admin Center"}</h2>
              <p>Select an item in the console tree. Double-click an object to open its properties, or right-click for actions.</p>
            </div>
          ) : (
            <table className="w-full border-collapse" onContextMenu={(e) => { e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY, item: null }); }}>
              <thead className="sticky top-0 bg-white dark:bg-[#252526]">
                <tr className="text-left">{["Name", "Type", "Description"].map((h) => <th key={h} className="border-b border-r border-[#e5e5e5] px-2 py-1 font-normal text-[#4d4d4d] dark:border-white/10 dark:text-[#bbb]">{h}</th>)}</tr>
              </thead>
              <tbody>
                {items.map((it) => (
                  <tr key={`${it.type}-${it.id}`} className={cx("cursor-default", picked === it.id ? "bg-[#cce8ff] dark:bg-[#264f78]" : "hover:bg-[#e5f3ff] dark:hover:bg-white/5", it.disabled && "text-ink-3")}
                    onClick={(e) => { e.stopPropagation(); setPicked(it.id); setMenu(null); }} onDoubleClick={() => activate(it)}
                    onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); setPicked(it.id); setMenu({ x: e.clientX, y: e.clientY, item: it }); }}>
                    <td className="px-2 py-[3px]"><span className="flex items-center gap-1.5">{it.icon}{it.name}</span></td>
                    <td className="px-2 py-[3px]">{it.typeLabel}</td>
                    <td className="truncate px-2 py-[3px] text-[#555] dark:text-[#aaa]">{it.description}</td>
                  </tr>
                ))}
                {items.length === 0 && <tr><td colSpan={3} className="px-2 py-6 text-center text-ink-3">There are no items to show in this view.</td></tr>}
              </tbody>
            </table>
          )}
        </div>
      </div>
      <div className="flex h-5 items-center border-t border-[#d9d9d9] bg-[#f0f0f0] px-2 text-[11px] text-[#555] dark:border-white/10 dark:bg-[#252525] dark:text-[#aaa]">
        {sel.kind === "page" ? loc.pathname : `${title}: ${items.length} object${items.length === 1 ? "" : "s"}`}
      </div>
      {menu && <div className="fixed inset-0 z-40" onClick={() => setMenu(null)} onContextMenu={(e) => { e.preventDefault(); setMenu(null); }}><MenuList items={actions(menu.item)} x={menu.x} y={menu.y} fixed onDone={() => setMenu(null)} /></div>}
      {dialog?.kind === "user-props" && <UserProps user={dialog.user} groups={groups.data ?? []} ous={ouList} onClose={() => setDialog(null)} onSaved={reload} />}
      {dialog?.kind === "group-props" && <GroupProps group={dialog.group} users={users.data ?? []} endpoints={eps.data ?? []} onClose={() => setDialog(null)} onSaved={reload} />}
      {(dialog?.kind === "new-user" || dialog?.kind === "new-group" || dialog?.kind === "new-ou") && <NewObject kind={dialog.kind} ou={currentOu} onClose={() => setDialog(null)} onSaved={reload} />}
      {dialog?.kind === "reset" && <ResetPassword user={dialog.user} onClose={() => setDialog(null)} />}
      {dialog?.kind === "delete" && (
        <Dialog open onClose={() => setDialog(null)} title="Confirm delete"
          footer={<><Button onClick={() => setDialog(null)}>No</Button><Button variant="danger" onClick={async () => {
            const it = dialog.item;
            const path = it.type === "user" ? `/users/${it.id}` : it.type === "group" ? `/groups/${it.id}` : it.type === "ou" ? `/ous/${it.id}` : it.type === "computer" ? `/endpoints/${it.id}` : `/policies/${it.id}`;
            try { await del(path); toast("good", `Deleted ${it.name}`); } catch (e) { toast("bad", (e as Error).message); }
            setDialog(null); reload();
          }}>Yes</Button></>}>
          <p className="text-sm">Are you sure you want to delete the {dialog.item.typeLabel} named "{dialog.item.name}"?</p>
        </Dialog>
      )}
    </div>
  );
}

function ToolBtn({ children, title, onClick, disabled }: { children: ReactNode; title: string; onClick: () => void; disabled?: boolean }) {
  return <button title={title} aria-label={title} disabled={disabled} onClick={onClick} className="grid size-6 place-items-center rounded border border-transparent hover:border-[#99c9ef] hover:bg-[#e5f3ff] disabled:opacity-35 dark:hover:bg-white/10">{children}</button>;
}

function MenuList({ items, x, y, fixed, onDone }: { items: { label: string; run: () => void; sep?: boolean; disabled?: boolean }[]; x: number; y: number; fixed?: boolean; onDone: () => void }) {
  return (
    <div className={cx(fixed ? "fixed" : "absolute", "z-50 min-w-48 border border-[#ccc] bg-[#f9f9f9] py-1 shadow-md dark:border-white/15 dark:bg-[#2b2b2b]")} style={{ left: x, top: y }} onClick={(e) => e.stopPropagation()}>
      {items.map((it, i) => it.sep ? <div key={i} className="my-1 h-px bg-[#ddd] dark:bg-white/10" /> : (
        <button key={i} disabled={it.disabled} className="block w-full px-6 py-1 text-left hover:bg-[#cce8ff] disabled:opacity-40 dark:hover:bg-[#264f78]" onClick={() => { onDone(); it.run(); }}>{it.label}</button>
      ))}
    </div>
  );
}

function UserProps({ user, groups, ous, onClose, onSaved }: { user: User; groups: Group[]; ous: OrgUnit[]; onClose: () => void; onSaved: () => void }) {
  const toast = useToast();
  const [tab, setTab] = useState<"general" | "account" | "member" | "org">("general");
  const [f, setF] = useState({ display_name: user.display_name, email: user.email, title: user.title, department: user.department, enabled: user.enabled, must_change_password: user.must_change_password });
  const [member, setMember] = useState<string[]>(user.group_ids);
  const save = async (close: boolean) => {
    try {
      await patch(`/users/${user.id}`, f);
      if (JSON.stringify(member) !== JSON.stringify(user.group_ids)) await put(`/users/${user.id}/groups`, { ids: member });
      toast("good", "Saved"); onSaved(); if (close) onClose();
    } catch (e) { toast("bad", (e as Error).message); }
  };
  return (
    <Dialog open onClose={onClose} title={`${user.display_name} Properties`}
      footer={<><Button variant="primary" onClick={() => save(true)}>OK</Button><Button onClick={onClose}>Cancel</Button><Button onClick={() => save(false)}>Apply</Button></>}>
      <Tabs value={tab} onChange={setTab} tabs={[{ id: "general", label: "General" }, { id: "account", label: "Account" }, { id: "member", label: "Member Of" }, { id: "org", label: "Organization" }]} />
      <div className="min-h-64 space-y-3 pt-3">
        {tab === "general" && <>
          <Field label="Display name"><Input value={f.display_name} onChange={(e) => setF({ ...f, display_name: e.target.value })} /></Field>
          <Field label="E-mail"><Input value={f.email} onChange={(e) => setF({ ...f, email: e.target.value })} /></Field>
        </>}
        {tab === "account" && <>
          <Field label="User logon name"><Input value={user.username} disabled /></Field>
          <Switch checked={!f.enabled} onChange={(v) => setF({ ...f, enabled: !v })} label="Account is disabled" />
          <Switch checked={f.must_change_password} onChange={(v) => setF({ ...f, must_change_password: v })} label="User must change password at next logon" />
          <p className="text-xs text-ink-3">OU: {ous.find((o) => o.id === user.ou_id)?.name ?? "Users (no OU)"} · Role: {user.role === "admin" ? "Domain Admin" : "User"}</p>
        </>}
        {tab === "member" && (
          <div className="max-h-64 overflow-auto rounded border border-line">
            {groups.map((g) => (
              <label key={g.id} className="flex items-center gap-2 border-b border-line px-2 py-1 text-sm last:border-0">
                <input type="checkbox" checked={member.includes(g.id)} onChange={(e) => setMember(e.target.checked ? [...member, g.id] : member.filter((x) => x !== g.id))} />{g.name}
              </label>
            ))}
          </div>
        )}
        {tab === "org" && <>
          <Field label="Job title"><Input value={f.title} onChange={(e) => setF({ ...f, title: e.target.value })} /></Field>
          <Field label="Department"><Input value={f.department} onChange={(e) => setF({ ...f, department: e.target.value })} /></Field>
        </>}
      </div>
    </Dialog>
  );
}

function GroupProps({ group, users, endpoints, onClose, onSaved }: { group: Group; users: User[]; endpoints: Endpoint[]; onClose: () => void; onSaved: () => void }) {
  const toast = useToast();
  const [tab, setTab] = useState<"general" | "members">("general");
  const [desc, setDesc] = useState(group.description);
  const [members, setMembers] = useState<string[]>(group.member_ids);
  const [computers, setComputers] = useState<string[]>(group.endpoint_ids);
  const save = async (close: boolean) => {
    try {
      await patch(`/groups/${group.id}`, { description: desc });
      await put(`/groups/${group.id}/members`, { ids: members });
      await put(`/groups/${group.id}/endpoints`, { ids: computers });
      toast("good", "Saved"); onSaved(); if (close) onClose();
    } catch (e) { toast("bad", (e as Error).message); }
  };
  return (
    <Dialog open onClose={onClose} title={`${group.name} Properties`}
      footer={<><Button variant="primary" onClick={() => save(true)}>OK</Button><Button onClick={onClose}>Cancel</Button><Button onClick={() => save(false)}>Apply</Button></>}>
      <Tabs value={tab} onChange={setTab} tabs={[{ id: "general", label: "General" }, { id: "members", label: "Members" }]} />
      <div className="min-h-64 space-y-3 pt-3">
        {tab === "general" && <><Field label="Group name"><Input value={group.name} disabled /></Field><Field label="Description"><Textarea rows={3} value={desc} onChange={(e) => setDesc(e.target.value)} /></Field><p className="text-xs text-ink-3">Group scope: Domain · Group type: Security</p></>}
        {tab === "members" && (
          <div className="max-h-72 overflow-auto rounded border border-line">
            {users.map((u) => <label key={u.id} className="flex items-center gap-2 border-b border-line px-2 py-1 text-sm"><input type="checkbox" checked={members.includes(u.id)} onChange={(e) => setMembers(e.target.checked ? [...members, u.id] : members.filter((x) => x !== u.id))} /><UserIcon className="size-3.5" />{u.display_name}</label>)}
            {endpoints.map((c) => <label key={c.id} className="flex items-center gap-2 border-b border-line px-2 py-1 text-sm"><input type="checkbox" checked={computers.includes(c.id)} onChange={(e) => setComputers(e.target.checked ? [...computers, c.id] : computers.filter((x) => x !== c.id))} /><Monitor className="size-3.5" />{c.hostname}</label>)}
          </div>
        )}
      </div>
    </Dialog>
  );
}

function NewObject({ kind, ou, onClose, onSaved }: { kind: "new-user" | "new-group" | "new-ou"; ou: string | null; onClose: () => void; onSaved: () => void }) {
  const toast = useToast();
  const [name, setName] = useState("");
  const [logon, setLogon] = useState("");
  const [pw, setPw] = useState("");
  const [desc, setDesc] = useState("");
  const title = kind === "new-user" ? "New Object - User" : kind === "new-group" ? "New Object - Group" : "New Object - Organizational Unit";
  const create = async () => {
    try {
      if (kind === "new-user") await post("/users", { username: logon, display_name: name, password: pw, ou_id: ou, must_change_password: true });
      if (kind === "new-group") await post("/groups", { name, description: desc, ou_id: ou });
      if (kind === "new-ou") await post("/ous", { name, description: desc, parent_id: ou });
      toast("good", `Created ${name || logon}`); onSaved(); onClose();
    } catch (e) { toast("bad", (e as Error).message); }
  };
  return (
    <Dialog open onClose={onClose} title={title} footer={<><Button variant="primary" onClick={create}>OK</Button><Button onClick={onClose}>Cancel</Button></>}>
      <div className="space-y-3">
        <Field label={kind === "new-user" ? "Full name" : "Name"}><Input autoFocus value={name} onChange={(e) => { setName(e.target.value); if (kind === "new-user" && !logon) setLogon(""); }} /></Field>
        {kind === "new-user" ? <>
          <Field label="User logon name"><Input value={logon} onChange={(e) => setLogon(e.target.value.toLowerCase())} placeholder={name.split(" ")[0]?.toLowerCase()} /></Field>
          <Field label="Password" hint="The user must change it at next logon."><Input type="password" value={pw} onChange={(e) => setPw(e.target.value)} /></Field>
        </> : <Field label="Description"><Input value={desc} onChange={(e) => setDesc(e.target.value)} /></Field>}
      </div>
    </Dialog>
  );
}

function ResetPassword({ user, onClose }: { user: User; onClose: () => void }) {
  const toast = useToast();
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [must, setMust] = useState(true);
  return (
    <Dialog open onClose={onClose} title="Reset Password"
      footer={<><Button variant="primary" disabled={!pw || pw !== pw2} onClick={async () => {
        try { await post(`/users/${user.id}/password`, { password: pw, must_change_password: must }); toast("good", `The password for ${user.display_name} has been changed.`); onClose(); } catch (e) { toast("bad", (e as Error).message); }
      }}>OK</Button><Button onClick={onClose}>Cancel</Button></>}>
      <div className="space-y-3">
        <Field label="New password"><Input type="password" autoFocus value={pw} onChange={(e) => setPw(e.target.value)} /></Field>
        <Field label="Confirm password"><Input type="password" value={pw2} onChange={(e) => setPw2(e.target.value)} /></Field>
        <Switch checked={must} onChange={setMust} label="User must change password at next logon" />
      </div>
    </Dialog>
  );
}
