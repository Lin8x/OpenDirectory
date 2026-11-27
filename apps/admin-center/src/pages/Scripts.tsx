// Script Library: your saved scripts, built-in templates and run history.
import { useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router-dom";
import { BookCopy, FileCode2, History, Plus, Terminal } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { post } from "../lib/api";
import type { OsFamily, Script, ScriptTemplate } from "../lib/types";
import { ago, osLabel } from "../lib/format";
import { Badge, Button, Card, EmptyState, Explain, Loading, OsIcon, PageHeader, Tabs, useToast } from "../components/ui";
import { JobList } from "./Jobs";
import { ListToolbar, NoMatches, useListView } from "../components/DataView";

type Tab = "library" | "templates" | "history";

export default function Scripts() {
  const [params, setParams] = useSearchParams();
  const tab = (params.get("tab") as Tab) || "library";
  const nav = useNavigate();
  return (
    <>
      <PageHeader title="Script Library" guide="09-everyday-tasks#run-something-on-many-computers" term="script_library"
        subtitle="Save scripts once, then run them on demand against endpoints, security groups or OUs, or attach them to a GPO as startup, sign-in or scheduled scripts."
        actions={<Button variant="primary" icon={<Plus className="size-4" />} onClick={() => nav("/scripts/new")}>New script</Button>} />
      <div className="mb-4">
        <Tabs value={tab} onChange={(t) => setParams({ tab: t })} tabs={[
          { id: "library", label: <span className="flex items-center gap-1.5"><BookCopy className="size-3.5" />My scripts</span> },
          { id: "templates", label: <span className="flex items-center gap-1.5"><FileCode2 className="size-3.5" />Templates</span> },
          { id: "history", label: <span className="flex items-center gap-1.5"><History className="size-3.5" />Run history</span> },
        ]} />
      </div>
      {tab === "library" && <Library onTemplates={() => setParams({ tab: "templates" })} />}
      {tab === "templates" && <Templates />}
      {tab === "history" && <RunHistory />}
      <div className="mt-5"><Explain>Scripts run with full system rights (SYSTEM on Windows, root on macOS and Linux) and time out after 30 minutes. Parameters reach the script as environment variables named OD_PARAM_&lt;NAME&gt;, so a value can never change the script itself.</Explain></div>
    </>
  );
}

function Library({ onTemplates }: { onTemplates: () => void }) {
  const scripts = useQuery<Script[]>("/scripts");
  const view = useListView(scripts.data, {
    id: "scripts",
    search: (s) => [s.name, s.description, s.category, s.shell, s.created_by],
    filters: [
      { kind: "enum", key: "category", label: "Category", get: (s) => s.category },
      { kind: "enum", key: "shell", label: "Shell", get: (s) => s.shell },
      { kind: "enum", key: "os", label: "OS", get: (s) => (s.os.length ? s.os : ["windows", "macos", "linux"]), format: (o) => osLabel[o as OsFamily] ?? o },
      { kind: "date", key: "updated", label: "Updated", get: (s) => s.updated_at },
    ],
    sorts: {
      name: { label: "Name", get: (s) => s.name },
      category: { label: "Category", get: (s) => s.category },
      updated: { label: "Updated", get: (s) => s.updated_at, type: "date" },
      created: { label: "Created", get: (s) => s.created_at, type: "date" },
    },
    defaultSort: { key: "name", dir: "asc" },
  });
  const list = view.rows;
  if (scripts.loading && !scripts.data) return <Loading />;
  if ((scripts.data ?? []).length === 0) {
    return <Card><EmptyState icon={<BookCopy className="size-5" />} title="Your library is empty" body="Start from one of the built-in templates, or write your own."
      action={<div className="flex gap-2"><Button variant="primary" onClick={onTemplates}>Browse templates</Button><Link to="/scripts/new"><Button>Write a script</Button></Link></div>} /></Card>;
  }
  return (
    <Card padded={false}>
      <ListToolbar view={view} placeholder="Search scripts" sortPicker noun={["script", "scripts"]} />
      {list.length === 0 && <NoMatches view={view} />}
      <ul className="divide-y divide-line">
        {list.map((s) => (
          <li key={s.id}>
            <Link to={`/scripts/${s.id}`} className="flex items-center gap-3 px-4 py-3 hover:bg-panel-2">
              <span className="grid size-8 place-items-center rounded-lg bg-accent-soft text-accent"><Terminal className="size-4" /></span>
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2"><span className="font-medium">{s.name}</span><Badge>{s.category}</Badge></div>
                <div className="truncate text-xs text-ink-3">{s.description || "No description"} · {s.shell} · updated {ago(s.updated_at)}</div>
              </div>
              <div className="flex gap-1">{(s.os.length ? s.os : (["windows", "macos", "linux"] as const)).map((o) => <OsIcon key={o} os={o} className="size-3.5" />)}</div>
            </Link>
          </li>
        ))}
      </ul>
    </Card>
  );
}

function Templates() {
  const tpls = useQuery<ScriptTemplate[]>("/scripts/templates");
  const nav = useNavigate();
  const toast = useToast();
  const [busy, setBusy] = useState<string | null>(null);
  const view = useListView(tpls.data, {
    id: "script-templates",
    search: (t) => [t.name, t.description, t.category, t.shell],
    filters: [
      { kind: "enum", key: "os", label: "OS", get: (t) => t.os, format: (o) => osLabel[o as OsFamily] ?? o },
      { kind: "enum", key: "category", label: "Category", get: (t) => t.category },
      { kind: "enum", key: "shell", label: "Shell", get: (t) => t.shell },
    ],
    sorts: { name: { label: "Name", get: (t) => t.name } },
    defaultSort: { key: "name", dir: "asc" },
  });
  if (!tpls.data) return <Loading />;
  const shown = view.rows;
  const cats = [...new Set(shown.map((t) => t.category))].sort();
  const use = async (t: ScriptTemplate) => {
    setBusy(t.key);
    try { const s = await post<Script>("/scripts", { template: t.key }); toast("good", `Added "${s.name}" to your library`); nav(`/scripts/${s.id}`); }
    catch (e) { toast("bad", (e as Error).message); } finally { setBusy(null); }
  };
  return (
    <div className="space-y-5">
      <Card padded={false}><ListToolbar view={view} placeholder="Search templates" sortPicker noun={["template", "templates"]} className="border-b-0" /></Card>
      {shown.length === 0 && <Card padded={false}><NoMatches view={view} /></Card>}
      {cats.map((c) => (
        <div key={c}>
          <h3 className="mb-2 text-sm font-semibold text-ink-2">{c}</h3>
          <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
            {shown.filter((t) => t.category === c).map((t) => (
              <div key={t.key} className="flex flex-col rounded-xl border border-line bg-panel p-4">
                <div className="flex items-center justify-between gap-2"><span className="text-sm font-semibold">{t.name}</span><div className="flex gap-1">{t.os.map((o) => <OsIcon key={o} os={o} className="size-3.5" />)}</div></div>
                <p className="mt-1 flex-1 text-xs leading-relaxed text-ink-3">{t.description}</p>
                <div className="mt-3 flex items-center justify-between">
                  <span className="text-[11px] text-ink-3">{t.shell}{t.params.length ? ` · ${t.params.length} parameter${t.params.length > 1 ? "s" : ""}` : ""}</span>
                  <Button size="sm" loading={busy === t.key} onClick={() => use(t)}>Use template</Button>
                </div>
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

export function RunHistory({ path = "/jobs?limit=200" }: { path?: string }) {
  return <JobList path={path} id={path === "/jobs?limit=200" ? "run-history" : "script-runs"}
    empty={<EmptyState icon={<History className="size-5" />} title="No runs yet" body="Runs from the library, from GPO scripts and from an endpoint's Remote scripts tab appear here." />} />;
}
