// Installed apps and packages, with install, update and uninstall through
// the endpoint's package manager (winget, Homebrew, apt, dnf, Flatpak…).
import { useState } from "react";
import { ArrowUpCircle, Download, Trash2 } from "lucide-react";
import { Link } from "react-router-dom";
import type { Endpoint, SoftwareInfo, SoftwareReport } from "../../lib/types";
import { ListToolbar, NoMatches, useListView } from "../DataView";
import { Badge, Button, Confirm, Dialog, Field, Input, Row, Select, Spinner, Table, Td } from "../ui";
import { LastOutput, ManageError, RefreshButton, useActions, useManage } from "./common";

export const MANAGER_LABEL: Record<string, string> = {
  winget: "winget", apt: "APT", dnf: "DNF", zypper: "Zypper", pacman: "pacman", brew: "Homebrew", brew_cask: "Homebrew Cask", flatpak: "Flatpak", snap: "Snap",
};
const SOURCE_LABEL: Record<string, string> = {
  programs: "Programs and Features", winget: "winget", apt: "APT (dpkg)", rpm: "RPM", pacman: "pacman", flatpak: "Flatpak", snap: "Snap", brew: "Homebrew", brew_cask: "Homebrew Cask", app: "Applications folder",
};
const LIMIT = 400;

export function SoftwarePanel({ endpoint }: { endpoint: Endpoint }) {
  const q = useManage<SoftwareReport>(endpoint, "software");
  const act = useActions(endpoint, () => void q.reload());
  const [remove, setRemove] = useState<SoftwareInfo | null>(null);
  const [install, setInstall] = useState(false);
  const view = useListView(q.data?.items, {
    id: `software.${endpoint.id}`,
    search: (s) => [s.name, s.package_id, s.publisher, s.version],
    filters: [
      { kind: "enum", key: "source", label: "Source", get: (s) => s.source, format: (v) => SOURCE_LABEL[v] ?? v },
      { kind: "toggle", key: "updates", label: "Updates available", test: (s) => !!s.update },
      { kind: "toggle", key: "managed", label: "Manageable", test: (s) => !!s.manager && !!s.package_id },
    ],
    sorts: {
      name: { label: "Name", get: (s) => s.name },
      version: { label: "Version", get: (s) => s.version },
      publisher: { label: "Publisher", get: (s) => s.publisher },
      source: { label: "Source", get: (s) => SOURCE_LABEL[s.source] ?? s.source },
    },
    defaultSort: { key: "name", dir: "asc" },
  });
  if (q.error && !q.data) return <ManageError error={q.error} onRetry={q.reload} />;
  const managers = q.data?.managers ?? [];
  const updates = (q.data?.items ?? []).filter((s) => s.update).length;
  const pkg = (s: SoftwareInfo, operation: "update" | "remove") =>
    act.run(`${s.package_id}:${operation}`, { op: "package", operation, name: s.package_id, manager: s.manager }, `${s.name} ${operation === "update" ? "updated" : "uninstalled"}`);

  return (
    <>
      <ListToolbar view={view} placeholder="Search apps and packages" noun={["app", "apps"]}
        extra={<>
          <RefreshButton loading={q.loading} onRefresh={q.reload} note={updates > 0 && <Badge tone="accent">{updates} update{updates === 1 ? "" : "s"} available</Badge>} />
          <Button size="sm" variant="primary" icon={<Download className="size-3.5" />} disabled={managers.length === 0} onClick={() => setInstall(true)}>Install app</Button>
        </>} />
      {q.data && q.data.notes.length > 0 && <div className="border-b border-line bg-warn/5 px-4 py-2 text-xs text-ink-2">{q.data.notes.join(" · ")}</div>}
      {!q.data ? <div className="grid place-items-center p-10"><Spinner /></div> : view.rows.length === 0 ? <NoMatches view={view} /> : (
        <>
          <Table view={view} head={[{ label: "Name", sort: "name" }, { label: "Version", sort: "version" }, { label: "Publisher", sort: "publisher" }, { label: "Source", sort: "source" }, ""]}>
            {view.rows.slice(0, LIMIT).map((s, i) => {
              const key = `${s.source}:${s.package_id || s.name}:${i}`;
              const busy = !!s.package_id && Object.keys(act.busy).some((k) => k.startsWith(`${s.package_id}:`));
              const manageable = !!s.manager && !!s.package_id;
              return (
                <Row key={key}>
                  <Td className="max-w-[380px]">
                    <div className="truncate font-medium">{s.name}</div>
                    {s.package_id && s.package_id !== s.name && <div className="truncate font-mono text-[11.5px] text-ink-3">{s.package_id}</div>}
                  </Td>
                  <Td className="font-mono text-[12px]">{s.version || "—"}{s.update && <div className="text-accent">→ {s.update}</div>}</Td>
                  <Td className="max-w-[220px] truncate text-[13px] text-ink-2">{s.publisher || "—"}</Td>
                  <Td className="text-[13px] text-ink-2">{SOURCE_LABEL[s.source] ?? s.source}</Td>
                  <Td className="whitespace-nowrap text-right">
                    {busy ? <Spinner /> : manageable && <>
                      {s.update && <Button size="sm" variant="ghost" icon={<ArrowUpCircle className="size-3.5" />} onClick={() => void pkg(s, "update")}>Update</Button>}
                      <Button size="sm" variant="ghost" icon={<Trash2 className="size-3.5" />} onClick={() => setRemove(s)}>Uninstall</Button>
                    </>}
                  </Td>
                </Row>
              );
            })}
          </Table>
          {view.rows.length > LIMIT && <p className="border-t border-line px-4 py-2.5 text-xs text-ink-3">Showing {LIMIT} of {view.rows.length}. Search or filter to narrow the list.</p>}
        </>
      )}
      <LastOutput job={act.last} onClose={act.clearLast} />
      <p className="border-t border-line px-4 py-2.5 text-xs text-ink-3">To install, update or remove apps on many endpoints at once, add a <Link to="/gpos" className="text-accent hover:underline">Software deployment</Link> setting to a GPO.</p>
      <Confirm open={!!remove} onClose={() => setRemove(null)} danger title={`Uninstall ${remove?.name}?`} confirmLabel="Uninstall"
        body={<>This runs {MANAGER_LABEL[remove?.manager ?? ""] ?? remove?.manager} to remove <span className="font-mono">{remove?.package_id}</span> from {endpoint.hostname}. It's recorded in the audit log.</>}
        onConfirm={async () => { if (remove) void pkg(remove, "remove"); }} />
      <InstallDialog open={install} onClose={() => setInstall(false)} managers={managers} os={endpoint.os}
        onInstall={(name, manager) => void act.run(`${name}:install`, { op: "package", operation: "install", name, manager }, `${name} installed`)} />
    </>
  );
}

const EXAMPLES: Record<string, string> = {
  winget: "Mozilla.Firefox", apt: "htop", dnf: "htop", zypper: "htop", pacman: "htop", brew: "wget", brew_cask: "firefox", flatpak: "org.mozilla.firefox", snap: "firefox",
};

function InstallDialog({ open, onClose, managers, onInstall }: { open: boolean; onClose: () => void; managers: string[]; os: string; onInstall: (name: string, manager: string) => void }) {
  const [name, setName] = useState("");
  const [manager, setManager] = useState("");
  const m = manager || managers[0] || "";
  return (
    <Dialog open={open} onClose={onClose} title="Install an app"
      footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!name.trim() || !m} onClick={() => { onInstall(name.trim(), m); setName(""); onClose(); }}>Install</Button></>}>
      <Field label="Package manager"><Select value={m} onChange={(e) => setManager(e.target.value)}>{managers.map((x) => <option key={x} value={x}>{MANAGER_LABEL[x] ?? x}</option>)}</Select></Field>
      <Field label="Package id" hint={m === "winget" ? "The winget id (winget search <name> finds it), for example Mozilla.Firefox." : `The package name for ${MANAGER_LABEL[m] ?? m}.`}>
        <Input className="font-mono text-[13px]" value={name} placeholder={EXAMPLES[m] ?? ""} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && name.trim()) { onInstall(name.trim(), m); setName(""); onClose(); } }} />
      </Field>
      <p className="text-xs text-ink-3">Installs run silently as SYSTEM or root, and the result shows up here and in the job history.</p>
    </Dialog>
  );
}
