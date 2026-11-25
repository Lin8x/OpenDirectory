import { ArrowRight, Monitor, Server, Terminal } from "lucide-react";
import { GLOSSARY } from "../lib/glossary";
import { Card, PageHeader } from "../components/ui";

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
  return (
    <>
      <PageHeader title="Learn" subtitle="How OpenDirectory works and how it maps to the Active Directory tools you may already know." />
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
