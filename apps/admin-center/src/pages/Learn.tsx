import { ArrowRight, BookOpen, ExternalLink, Monitor, PlayCircle, Server, Terminal } from "lucide-react";
import { useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { GLOSSARY } from "../lib/glossary";
import { KB, type KbEntry, type KbLink, type KbVideo } from "../lib/kb";
import { Badge, Card, Input, PageHeader, Tabs, cx } from "../components/ui";

const ORDER = ["domain", "management_server", "ou", "user", "group", "domain_admin", "endpoint", "agent", "enrollment", "fingerprint", "gpo", "priority", "rsop", "local_accounts", "compliance", "rmm"];

const AD_MAP: [string, string][] = [
  ["Active Directory Users and Computers", "Users, Security Groups, Organizational Units"],
  ["Group Policy Management Console", "Group Policy"],
  ["gpresult / RSoP", "Endpoint > Policy & compliance"],
  ["Domain join", "Enrollment (works on Windows Home, macOS and Linux too)"],
  ["Restricted Groups / domain logon", "Sign-in accounts setting"],
  ["PowerShell remoting / SCCM scripts", "Remote Scripts"],
  ["Event Viewer (Security log, changes)", "Audit Log"],
  ["ActiveDirectory PowerShell module", "od CLI and the REST API"],
];

export default function Learn() {
  const [params, setParams] = useSearchParams();
  const tab = params.get("tab") === "kb" ? "kb" : "concepts";
  return (
    <>
      <PageHeader title="Learn" subtitle="How OpenDirectory works, the networking and identity concepts behind it, and their Active Directory counterparts." />
      <div className="mb-5"><Tabs value={tab} onChange={(t) => setParams(t === "kb" ? { tab: "kb" } : {})} tabs={[{ id: "concepts", label: "OpenDirectory concepts" }, { id: "kb", label: "Knowledge Base" }]} /></div>
      {tab === "kb" ? <KnowledgeBase /> : <Concepts />}
    </>
  );
}

function Concepts() {
  return (
    <>
      <Card title="How the pieces fit together" className="mb-5">
        <div className="grid items-center gap-3 md:grid-cols-[1fr_auto_1fr_auto_1fr]">
          <Piece icon={<Monitor className="size-5" />} title="Admin Center & od CLI" body="Where admins work. This app, or the od command on any OS." />
          <ArrowRight className="mx-auto hidden size-5 text-ink-3 md:block" />
          <Piece icon={<Server className="size-5" />} title="Management Server" body="Stores the directory and policies. Runs inside this app or on a dedicated server." />
          <ArrowRight className="mx-auto hidden size-5 text-ink-3 md:block" />
          <Piece icon={<Terminal className="size-5" />} title="Endpoint Agents" body="A background service on every Windows, macOS and Linux computer." />
        </div>
        <p className="mt-4 text-sm leading-relaxed text-ink-2">Agents always connect out to the Management Server over an encrypted, pinned connection, so endpoints behind routers or on home networks need no open ports. Everything stays on your network.</p>
      </Card>
      <Card title="Coming from Active Directory?" className="mb-5" padded={false}>
        <table className="w-full text-sm">
          <thead><tr className="border-b border-line text-left text-xs uppercase tracking-wide text-ink-3"><th className="px-4 py-2 font-medium">In Windows Server</th><th className="px-4 py-2 font-medium">In OpenDirectory</th></tr></thead>
          <tbody className="divide-y divide-line">{AD_MAP.map(([a, b]) => <tr key={a}><td className="px-4 py-2.5 text-ink-2">{a}</td><td className="px-4 py-2.5 font-medium">{b}</td></tr>)}</tbody>
        </table>
      </Card>
      <div className="grid gap-4 md:grid-cols-2">
        {ORDER.map((k) => GLOSSARY[k]).filter(Boolean).map((t) => (
          <Card key={t!.term}>
            <div className="flex items-baseline justify-between gap-2"><h3 className="font-semibold">{t!.term}</h3>{t!.adName && <span className="text-xs text-ink-3">AD: {t!.adName}</span>}</div>
            <p className="mt-1 text-sm font-medium text-ink-2">{t!.short}</p>
            <p className="mt-2 text-sm leading-relaxed text-ink-2">{t!.long}</p>
          </Card>
        ))}
      </div>
    </>
  );
}

function Piece({ icon, title, body }: { icon: React.ReactNode; title: string; body: string }) {
  return (
    <div className="rounded-xl border border-line bg-panel-2 p-4">
      <div className="grid size-9 place-items-center rounded-lg bg-accent-soft text-accent">{icon}</div>
      <div className="mt-2.5 text-sm font-semibold">{title}</div>
      <p className="mt-1 text-xs leading-relaxed text-ink-3">{body}</p>
    </div>
  );
}

function ExtLink({ l }: { l: KbLink }) {
  return <a href={l.url} target="_blank" rel="noreferrer" className="flex items-center gap-1.5 text-sm text-accent hover:underline"><ExternalLink className="size-3.5 shrink-0" />{l.label}</a>;
}

function KnowledgeBase() {
  const [params, setParams] = useSearchParams();
  const [q, setQ] = useState("");
  const list = KB.filter((e) => !q || `${e.title} ${e.summary} ${e.microsoft?.name ?? ""} ${e.standard.text.join(" ")}`.toLowerCase().includes(q.toLowerCase()));
  const cur = KB.find((e) => e.key === params.get("topic")) ?? list[0] ?? KB[0]!;
  const cats = [...new Set(KB.map((e) => e.category))];
  return (
    <div className="grid gap-5 lg:grid-cols-[260px_minmax(0,1fr)]">
      <Card padded={false} className="self-start">
        <div className="border-b border-line p-2"><Input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search topics…" aria-label="Search topics" /></div>
        <nav className="max-h-[70vh] overflow-y-auto p-1.5">
          {cats.map((c) => {
            const items = list.filter((e) => e.category === c);
            if (!items.length) return null;
            return (
              <div key={c} className="mb-1">
                <div className="px-2 pb-1 pt-2 text-[11px] font-semibold uppercase tracking-wide text-ink-3">{c}</div>
                {items.map((e) => (
                  <button key={e.key} onClick={() => setParams({ tab: "kb", topic: e.key })} className={cx("w-full rounded-md px-2 py-1.5 text-left text-sm", e.key === cur.key ? "bg-accent-soft font-medium text-accent" : "hover:bg-panel-2")}>{e.title}</button>
                ))}
              </div>
            );
          })}
        </nav>
      </Card>
      <Topic key={cur.key} e={cur} />
    </div>
  );
}

function VideoCard({ v, label }: { v: KbVideo; label?: string }) {
  return (
    <a href={v.url} target="_blank" rel="noreferrer" className="flex items-start gap-3 rounded-lg border border-line p-3 hover:bg-panel-2">
      <PlayCircle className="mt-0.5 size-6 shrink-0 text-bad" />
      <span><span className="block text-sm font-medium">{v.title}</span><span className="text-xs text-ink-3">{label ?? "YouTube"} · {v.channel}</span></span>
    </a>
  );
}

function WikiCard({ l }: { l: KbLink }) {
  return (
    <a href={l.url} target="_blank" rel="noreferrer" className="flex items-start gap-3 rounded-lg border border-line p-3 hover:bg-panel-2">
      <span className="grid size-6 shrink-0 place-items-center rounded font-serif text-base font-bold">W</span>
      <span><span className="block text-sm font-medium">{l.label}</span><span className="text-xs text-ink-3">Wikipedia</span></span>
    </a>
  );
}

function Resources({ wikipedia, video, links }: { wikipedia?: KbLink; video?: KbVideo; links: KbLink[] }) {
  if (!wikipedia && !video && links.length === 0) return null;
  return (
    <div className="mt-5 border-t border-line pt-4">
      <div className="mb-3 text-xs font-semibold uppercase tracking-wide text-ink-3">Learn more</div>
      {(video || wikipedia) && <div className="grid gap-3 md:grid-cols-2">{video && <VideoCard v={video} />}{wikipedia && <WikiCard l={wikipedia} />}</div>}
      {links.length > 0 && <div className="mt-3 space-y-1.5">{links.map((l) => <ExtLink key={l.url} l={l} />)}</div>}
    </div>
  );
}

/** The featured Microsoft Learn documentation card. */
function LearnCard({ l }: { l: KbLink }) {
  return (
    <a href={l.url} target="_blank" rel="noreferrer" className="flex items-start gap-3 rounded-lg border border-line p-3 hover:bg-panel-2">
      <BookOpen className="mt-0.5 size-6 shrink-0 text-accent" />
      <span><span className="block text-sm font-medium">{l.label.replace(/ \(Microsoft Learn\)$/, "")}</span><span className="text-xs text-ink-3">Microsoft Learn · official documentation</span></span>
    </a>
  );
}

const PAGE_NAMES: Record<string, string> = {
  "/users": "Users", "/network-services": "Network Services", "/domain-controller": "Domain Controller", "/gpos": "Group Policy",
  "/lab": "Network Lab", "/patches": "Patch Management", "/endpoints": "Endpoints", "/mobile": "Mobile Devices", "/backups": "Backup & Restore",
};

type View = "standard" | "ours" | "microsoft";

function Topic({ e }: { e: KbEntry }) {
  const [view, setView] = useState<View>("standard");
  const tabs: { id: View; label: string }[] = [{ id: "standard", label: "The standard" }, { id: "ours", label: "In OpenDirectory" }];
  if (e.microsoft) tabs.push({ id: "microsoft", label: "Microsoft / AD" });
  const para = (t: string[]) => t.map((x) => <p key={x} className="text-sm leading-relaxed text-ink-2">{x}</p>);
  return (
    <Card className="min-w-0">
      <div className="flex flex-wrap items-center gap-2"><h2 className="text-lg font-semibold">{e.title}</h2><Badge>{e.category}</Badge></div>
      <p className="mt-1 text-sm text-ink-2">{e.summary}</p>
      <div className="mt-4"><Tabs value={view} onChange={setView} tabs={tabs} /></div>
      <div className="mt-4 space-y-3">
        {view === "standard" && <>{para(e.standard.text)}<Resources wikipedia={e.standard.wikipedia} video={e.standard.video} links={e.standard.links} /></>}
        {view === "ours" && <>
          {para(e.ours.text)}
          {e.ours.to && (
            <div className="mt-5 border-t border-line pt-4">
              <Link to={e.ours.to} className="flex max-w-sm items-start gap-3 rounded-lg border border-line p-3 hover:bg-panel-2">
                <ArrowRight className="mt-0.5 size-6 shrink-0 text-accent" />
                <span><span className="block text-sm font-medium">Open {PAGE_NAMES[e.ours.to] ?? "it"}</span><span className="text-xs text-ink-3">In the Admin Center</span></span>
              </Link>
            </div>
          )}
          <Resources links={e.ours.links} />
        </>}
        {view === "microsoft" && e.microsoft && <>
          <h3 className="font-semibold">{e.microsoft.name}</h3>
          {para(e.microsoft.text)}
          <div className="mt-5 border-t border-line pt-4">
            <div className="mb-3 text-xs font-semibold uppercase tracking-wide text-ink-3">Learn more</div>
            <div className="grid gap-3 md:grid-cols-2">
              <LearnCard l={e.microsoft.learn} />
              <VideoCard v={e.microsoft.video} label="YouTube · Windows Server how-to" />
            </div>
          </div>
        </>}
      </div>
    </Card>
  );
}
