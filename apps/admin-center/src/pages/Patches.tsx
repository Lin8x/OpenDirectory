// Patch Management: pending OS updates on every endpoint, with scan and
// install actions for endpoints, groups or OUs.
import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { ChevronDown, ChevronRight, Download, RefreshCw, RotateCw, ShieldAlert } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { post } from "../lib/api";
import { ago } from "../lib/format";
import type { Endpoint, Group, OrgUnit, RunTargets } from "../lib/types";
import { Badge, Button, Card, Dialog, EmptyState, Explain, Loading, OsIcon, PageHeader, Switch, useToast } from "../components/ui";
import { NO_TARGETS, TargetPicker } from "../components/TargetPicker";

export default function Patches() {
  const eps = useQuery<Endpoint[]>("/endpoints", { poll: 20000 });
  const groups = useQuery<Group[]>("/groups");
  const ous = useQuery<OrgUnit[]>("/ous");
  const [open, setOpen] = useState<string | null>(null);
  const [dialog, setDialog] = useState<"scan" | "install" | null>(null);
  const list = useMemo(() => [...(eps.data ?? [])].sort((a, b) => (b.inventory.patches?.pending.length ?? -1) - (a.inventory.patches?.pending.length ?? -1)), [eps.data]);
  const scanned = list.filter((e) => e.inventory.patches?.scanned_at && !e.inventory.patches.error);
  const pending = scanned.reduce((a, e) => a + e.inventory.patches!.pending.length, 0);
  const security = scanned.reduce((a, e) => a + e.inventory.patches!.pending.filter((p) => p.security).length, 0);
  const reboot = scanned.filter((e) => e.inventory.patches!.reboot_required).length;
  const upToDate = scanned.filter((e) => e.inventory.patches!.pending.length === 0).length;
  return (
    <>
      <PageHeader title="Patch Management" guide="09-everyday-tasks#install-operating-system-updates" term="patching" subtitle="Pending OS updates on every endpoint from Windows Update, apt, dnf and softwareupdate. Scans run every 6 hours."
        actions={<div className="flex gap-2"><Button icon={<RefreshCw className="size-4" />} onClick={() => setDialog("scan")}>Scan now…</Button><Button variant="primary" icon={<Download className="size-4" />} onClick={() => setDialog("install")}>Install updates…</Button></div>} />
      <div className="mb-5 grid gap-3 sm:grid-cols-4">
        <Tile label="Up to date" value={`${upToDate} / ${list.length}`} />
        <Tile label="Pending updates" value={pending} />
        <Tile label="Security updates" value={security} tone={security ? "bad" : undefined} />
        <Tile label="Need a restart" value={reboot} tone={reboot ? "warn" : undefined} />
      </div>
      <Card padded={false}>
        {eps.loading && !eps.data ? <Loading /> : list.length === 0 ? <EmptyState icon={<ShieldAlert className="size-5" />} title="No endpoints yet" body="Enroll endpoints to see their pending updates here." /> : (
          <ul className="divide-y divide-line">
            {list.map((e) => {
              const r = e.inventory.patches;
              const sec = r?.pending.filter((p) => p.security).length ?? 0;
              return (
                <li key={e.id}>
                  <button className="flex w-full items-center gap-3 px-4 py-2.5 text-left hover:bg-panel-2" onClick={() => setOpen(open === e.id ? null : e.id)}>
                    {open === e.id ? <ChevronDown className="size-4 text-ink-3" /> : <ChevronRight className="size-4 text-ink-3" />}
                    <OsIcon os={e.os} />
                    <Link to={`/endpoints/${e.id}`} onClick={(x) => x.stopPropagation()} className="w-44 truncate font-medium hover:underline">{e.hostname}</Link>
                    <span className="flex flex-1 flex-wrap items-center gap-1.5 text-xs">
                      {!r ? <span className="text-ink-3">Not scanned yet (agent 0.5 or newer)</span> : r.error ? <Badge tone="warn">{r.error}</Badge> : r.pending.length === 0 ? <Badge tone="good">Up to date</Badge> : <>
                        <Badge>{r.pending.length} pending</Badge>{sec > 0 && <Badge tone="bad">{sec} security</Badge>}
                      </>}
                      {r?.reboot_required && <Badge tone="warn"><span className="flex items-center gap-1"><RotateCw className="size-3" />Restart needed</span></Badge>}
                    </span>
                    <span className="text-xs text-ink-3">{r?.source}{r?.scanned_at ? ` · ${ago(r.scanned_at)}` : ""}</span>
                  </button>
                  {open === e.id && r && r.pending.length > 0 && (
                    <ul className="max-h-72 overflow-y-auto border-t border-line bg-panel-2/40 px-12 py-2 text-xs">
                      {r.pending.map((p, i) => <li key={i} className="flex gap-2 py-0.5">{p.security && <Badge tone="bad" className="py-0">Security</Badge>}{p.severity && <Badge className="py-0">{p.severity}</Badge>}<span>{p.title}</span><span className="text-ink-3">{p.id}</span></li>)}
                    </ul>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </Card>
      <div className="mt-5"><Explain>Installs run as SYSTEM/root and can take a while; follow them in the Script Library's run history. For patch rings, combine the Update deferral GPO setting with a Pilot OU, or install here on the Pilot OU first.</Explain></div>
      {dialog && <RunDialog mode={dialog} endpoints={eps.data ?? []} groups={groups.data ?? []} ous={ous.data ?? []} onClose={() => setDialog(null)} />}
    </>
  );
}

function Tile({ label, value, tone }: { label: string; value: number | string; tone?: "bad" | "warn" }) {
  return <div className="rounded-xl border border-line bg-panel p-4"><div className="text-xs text-ink-3">{label}</div><div className={`mt-1 text-2xl font-semibold ${tone === "bad" ? "text-bad" : tone === "warn" ? "text-warn" : ""}`}>{value}</div></div>;
}

function RunDialog({ mode, endpoints, groups, ous, onClose }: { mode: "scan" | "install"; endpoints: Endpoint[]; groups: Group[]; ous: OrgUnit[]; onClose: () => void }) {
  const toast = useToast();
  const [targets, setTargets] = useState<RunTargets>({ ...NO_TARGETS, all_endpoints: mode === "scan" });
  const [securityOnly, setSecurityOnly] = useState(true);
  const [reboot, setReboot] = useState(false);
  const [busy, setBusy] = useState(false);
  return (
    <Dialog open onClose={onClose} wide title={mode === "scan" ? "Scan for updates" : "Install updates"}
      footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" loading={busy} onClick={async () => {
        setBusy(true);
        try { const j = await post<unknown[]>("/patches/run", { targets, install: mode === "install", security_only: securityOnly, reboot }); toast("good", `Queued on ${j.length} endpoint(s)`); onClose(); }
        catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
      }}>{mode === "scan" ? "Scan" : "Install"}</Button></>}>
      <div className="space-y-4">
        <TargetPicker value={targets} onChange={setTargets} endpoints={endpoints} groups={groups} ous={ous} />
        {mode === "install" && <>
          <Switch checked={securityOnly} onChange={setSecurityOnly} label="Security updates only" />
          <Switch checked={reboot} onChange={setReboot} label="Restart afterwards if an update needs it (users get a 1-minute warning)" />
        </>}
      </div>
    </Dialog>
  );
}
