// Containers: a small built-in Portainer for Docker or Podman. Containers
// and images, logs, a console (exec), start/stop/restart and update.
import { useEffect, useState } from "react";
import { Box, Download, ExternalLink, FileText, Link2, Pause, Play, RefreshCcw, RotateCw, Square, SquareTerminal, Trash2, Wand2 } from "lucide-react";
import { get, put } from "../../lib/api";
import { useQuery } from "../../lib/hooks";
import { useSession } from "../../lib/session";
import type { ContainerControl, ContainerInfo, ContainerReport, Endpoint, ImageInfo } from "../../lib/types";
import { ListToolbar, NoMatches, useListView } from "../DataView";
import { RemoteShell } from "../RemoteShell";
import { Badge, Button, Callout, Confirm, Dialog, EmptyState, Field, Input, Row, Select, Spinner, Table, Tabs, Td, useToast } from "../ui";
import { LastOutput, ManageError, RefreshButton, StateBadge, useActions, useManage } from "./common";

export function ContainersPanel({ endpoint }: { endpoint: Endpoint }) {
  const q = useManage<ContainerReport>(endpoint, "containers", 10000);
  const act = useActions(endpoint, () => void q.reload());
  const [tab, setTab] = useState<"containers" | "images">("containers");
  const [logs, setLogs] = useState<ContainerInfo | null>(null);
  const [consoleFor, setConsole] = useState<ContainerInfo | null>(null);
  const [confirm, setConfirm] = useState<{ title: string; body: string; label: string; run: () => void } | null>(null);
  const [pull, setPull] = useState(false);
  const portainer = useQuery<{ url: string }>("/portainer");
  const [linkOpen, setLinkOpen] = useState(false);
  const admin = useSession().me?.user.role === "admin";

  if (q.error && !q.data) return <ManageError error={q.error} onRetry={q.reload} />;
  if (!q.data) return <div className="grid place-items-center p-10"><Spinner /></div>;
  const d = q.data;
  if (!d.runtime) {
    return <EmptyState icon={<Box className="size-6" />} title="No container runtime" body={d.error || "Install Docker or Podman on this endpoint to manage containers here."} />;
  }
  const control = (c: ContainerInfo, control: ContainerControl, done: string) => act.run(`${c.id}:${control}`, { op: "container", id: c.id, control }, done);
  const portainerUrl = portainer.data?.url;

  return (
    <>
      <div className="flex flex-wrap items-center gap-2 border-b border-line px-4 py-2.5">
        <Badge tone="accent" className="capitalize">{d.runtime}</Badge>
        {d.version && <span className="text-xs text-ink-3">version {d.version}</span>}
        <span className="text-xs text-ink-3">· {d.containers.filter((c) => c.state === "running").length} running of {d.containers.length} · {d.images.length} images</span>
        <div className="ml-auto flex flex-wrap items-center gap-1.5">
          <Button size="sm" icon={<Download className="size-3.5" />} onClick={() => setPull(true)}>Pull image</Button>
          <Button size="sm" icon={<Wand2 className="size-3.5" />} loading={!!act.busy.prune}
            onClick={() => setConfirm({ title: "Prune unused objects?", label: "Prune", body: "Removes stopped containers, dangling images and unused networks on this endpoint. Volumes are kept.", run: () => void act.run("prune", { op: "prune" }, "Pruned unused containers, images and networks") })}>Prune</Button>
          {portainerUrl && <a href={portainerUrl} target="_blank" rel="noreferrer" className="inline-flex h-7 items-center gap-1.5 rounded-md border border-line bg-panel px-2.5 text-[13px] font-medium hover:bg-panel-2"><ExternalLink className="size-3.5" />Open Portainer</a>}
          {admin && <Button size="sm" variant="ghost" icon={<Link2 className="size-3.5" />} onClick={() => setLinkOpen(true)}>{portainerUrl ? "Portainer link" : "Link Portainer"}</Button>}
        </div>
      </div>
      <div className="px-4 pt-2"><Tabs value={tab} onChange={setTab} tabs={[{ id: "containers", label: `Containers (${d.containers.length})` }, { id: "images", label: `Images (${d.images.length})` }]} /></div>
      {tab === "containers" ? (
        <ContainerTable endpoint={endpoint} rows={d.containers} loading={q.loading} reload={q.reload} busy={act.busy}
          onControl={(c, ctl) => {
            if (ctl === "stop") setConfirm({ title: `Stop ${c.name}?`, label: "Stop", body: "Whatever this container serves is unavailable until it's started again.", run: () => void control(c, "stop", `${c.name} stopped`) });
            else if (ctl === "remove") setConfirm({ title: `Remove ${c.name}?`, label: "Remove container", body: "The container is deleted (forced if it's running). Named volumes are kept; anything stored only inside the container is lost.", run: () => void control(c, "remove", `${c.name} removed`) });
            else if (ctl === "update") setConfirm({ title: `Update ${c.name}?`, label: "Pull and recreate", body: `Pulls the newest ${c.image} and recreates the container with the same ports, volumes, environment and restart policy${c.project ? ` (through Docker Compose, project ${c.project})` : ""}. If recreating fails, the old container is put back.`, run: () => void control(c, "update", `${c.name} updated`) });
            else void control(c, ctl, `${c.name}: ${ctl} done`);
          }}
          onLogs={setLogs} onConsole={setConsole} />
      ) : (
        <ImageTable endpoint={endpoint} rows={d.images} loading={q.loading} reload={q.reload} busy={act.busy}
          onRemove={(i) => setConfirm({ title: `Remove ${i.repository}:${i.tag}?`, label: "Remove image", body: "The image is deleted from this endpoint. Containers that use it must be removed first.", run: () => void act.run(`${i.id}:rmi`, { op: "remove_image", id: i.repository !== "<none>" ? `${i.repository}:${i.tag}` : i.id }, "Image removed") })} />
      )}
      <LastOutput job={act.last} onClose={act.clearLast} />
      <Confirm open={!!confirm} onClose={() => setConfirm(null)} danger title={confirm?.title ?? ""} confirmLabel={confirm?.label} body={confirm?.body} onConfirm={async () => confirm?.run()} />
      {logs && <LogsDialog endpoint={endpoint} c={logs} onClose={() => setLogs(null)} />}
      {consoleFor && <ConsoleDialog endpoint={endpoint} c={consoleFor} onClose={() => setConsole(null)} />}
      <PullDialog open={pull} onClose={() => setPull(false)} onPull={(image) => void act.run(`pull:${image}`, { op: "pull_image", image }, `Pulled ${image}`)} />
      {linkOpen && <PortainerDialog current={portainerUrl ?? ""} onClose={() => setLinkOpen(false)} onSaved={() => void portainer.reload()} />}
    </>
  );
}

function ContainerTable({ endpoint, rows, loading, reload, busy, onControl, onLogs, onConsole }: {
  endpoint: Endpoint; rows: ContainerInfo[]; loading: boolean; reload: () => void; busy: Record<string, boolean>;
  onControl: (c: ContainerInfo, ctl: ContainerControl) => void; onLogs: (c: ContainerInfo) => void; onConsole: (c: ContainerInfo) => void;
}) {
  const view = useListView(rows, {
    id: `containers.${endpoint.id}`,
    search: (c) => [c.name, c.image, c.id, c.ports, c.project],
    filters: [
      { kind: "enum", key: "state", label: "State", get: (c) => c.state },
      { kind: "enum", key: "project", label: "Stack", get: (c) => c.project || null },
    ],
    sorts: { name: { label: "Name", get: (c) => c.name }, image: { label: "Image", get: (c) => c.image }, state: { label: "State", get: (c) => c.state } },
    defaultSort: { key: "name", dir: "asc" },
  });
  return (
    <>
      <ListToolbar view={view} placeholder="Search containers" noun={["container", "containers"]} extra={<RefreshButton loading={loading} onRefresh={reload} />} />
      {rows.length === 0 ? <p className="p-6 text-center text-sm text-ink-3">No containers on this endpoint yet.</p> : view.rows.length === 0 ? <NoMatches view={view} /> : (
        <Table view={view} head={[{ label: "Name", sort: "name" }, { label: "State", sort: "state" }, { label: "Image", sort: "image" }, "Ports", ""]}>
          {view.rows.map((c) => {
            const isBusy = Object.keys(busy).some((k) => k.startsWith(`${c.id}:`));
            const running = c.state === "running", paused = c.state === "paused";
            return (
              <Row key={c.id}>
                <Td>
                  <div className="font-medium">{c.name}</div>
                  <div className="font-mono text-[11px] text-ink-3">{c.id.slice(0, 12)}{c.project && <> · stack <span className="text-ink-2">{c.project}</span></>}</div>
                </Td>
                <Td><StateBadge state={c.state} /><div className="text-[11px] text-ink-3">{c.status}</div></Td>
                <Td className="max-w-[260px] truncate font-mono text-[12px]" >{c.image}</Td>
                <Td className="max-w-[220px] truncate font-mono text-[11.5px] text-ink-2" >{c.ports || "—"}</Td>
                <Td className="whitespace-nowrap text-right">
                  {isBusy ? <Spinner /> : <div className="flex justify-end gap-0.5">
                    {!running && !paused && <IconBtn title="Start" onClick={() => onControl(c, "start")}><Play className="size-3.5" /></IconBtn>}
                    {running && <IconBtn title="Stop" onClick={() => onControl(c, "stop")}><Square className="size-3.5" /></IconBtn>}
                    {(running || paused) && <IconBtn title="Restart" onClick={() => onControl(c, "restart")}><RotateCw className="size-3.5" /></IconBtn>}
                    {running && <IconBtn title="Pause" onClick={() => onControl(c, "pause")}><Pause className="size-3.5" /></IconBtn>}
                    {paused && <IconBtn title="Resume (unpause)" onClick={() => onControl(c, "unpause")}><Play className="size-3.5" /></IconBtn>}
                    <IconBtn title="Logs" onClick={() => onLogs(c)}><FileText className="size-3.5" /></IconBtn>
                    {running && <IconBtn title="Console (exec)" onClick={() => onConsole(c)}><SquareTerminal className="size-3.5" /></IconBtn>}
                    <IconBtn title="Update: pull the image and recreate" onClick={() => onControl(c, "update")}><RefreshCcw className="size-3.5" /></IconBtn>
                    <IconBtn title="Remove" onClick={() => onControl(c, "remove")}><Trash2 className="size-3.5" /></IconBtn>
                  </div>}
                </Td>
              </Row>
            );
          })}
        </Table>
      )}
    </>
  );
}

function ImageTable({ endpoint, rows, loading, reload, busy, onRemove }: { endpoint: Endpoint; rows: ImageInfo[]; loading: boolean; reload: () => void; busy: Record<string, boolean>; onRemove: (i: ImageInfo) => void }) {
  const view = useListView(rows, {
    id: `images.${endpoint.id}`,
    search: (i) => [i.repository, i.tag, i.id],
    filters: [{ kind: "toggle", key: "dangling", label: "Dangling", test: (i) => i.repository === "<none>" }],
    sorts: { repo: { label: "Repository", get: (i) => i.repository }, size: { label: "Size", get: (i) => i.size }, created: { label: "Created", get: (i) => i.created } },
    defaultSort: { key: "repo", dir: "asc" },
  });
  return (
    <>
      <ListToolbar view={view} placeholder="Search images" noun={["image", "images"]} extra={<RefreshButton loading={loading} onRefresh={reload} />} />
      {view.rows.length === 0 ? <NoMatches view={view} /> : (
        <Table view={view} head={[{ label: "Repository", sort: "repo" }, "Tag", "Image ID", { label: "Size", sort: "size" }, { label: "Created", sort: "created" }, ""]}>
          {view.rows.map((i) => (
            <Row key={`${i.id}:${i.repository}:${i.tag}`}>
              <Td className="font-mono text-[12.5px]">{i.repository}</Td>
              <Td className="font-mono text-[12px]">{i.tag || "—"}</Td>
              <Td className="font-mono text-[11.5px] text-ink-3">{i.id.replace(/^sha256:/, "").slice(0, 12)}</Td>
              <Td className="text-[13px]">{i.size}</Td>
              <Td className="text-[13px] text-ink-2">{i.created}</Td>
              <Td className="text-right">{busy[`${i.id}:rmi`] ? <Spinner /> : <IconBtn title="Remove image" onClick={() => onRemove(i)}><Trash2 className="size-3.5" /></IconBtn>}</Td>
            </Row>
          ))}
        </Table>
      )}
    </>
  );
}

function IconBtn({ title, onClick, children }: { title: string; onClick: () => void; children: React.ReactNode }) {
  return <button type="button" title={title} aria-label={title} onClick={onClick} className="rounded-md p-1.5 text-ink-3 transition hover:bg-panel-2 hover:text-ink">{children}</button>;
}

function LogsDialog({ endpoint, c, onClose }: { endpoint: Endpoint; c: ContainerInfo; onClose: () => void }) {
  const [tail, setTail] = useState(500);
  const [text, setText] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const load = async () => {
    setLoading(true);
    try { const r = await get<{ logs: string }>(`/endpoints/${endpoint.id}/manage/containers/${encodeURIComponent(c.id)}/logs?tail=${tail}`); setText(r.logs); setErr(null); }
    catch (e) { setErr((e as Error).message); } finally { setLoading(false); }
  };
  useEffect(() => { void load(); }, [tail]); // eslint-disable-line react-hooks/exhaustive-deps
  return (
    <Dialog open onClose={onClose} wide title={`Logs: ${c.name}`}
      footer={<><Select className="h-8 w-40" value={tail} onChange={(e) => setTail(Number(e.target.value))}>{[100, 500, 2000, 5000].map((n) => <option key={n} value={n}>Last {n} lines</option>)}</Select>
        <Button loading={loading} onClick={() => void load()}>Refresh</Button><Button onClick={onClose}>Close</Button></>}>
      {err && <Callout tone="warn">{err}</Callout>}
      {text === null && !err ? <div className="grid place-items-center p-8"><Spinner /></div>
        : <pre className="selectable max-h-[60vh] overflow-auto whitespace-pre-wrap rounded-md bg-[#0f1218] p-3 font-mono text-[11.5px] leading-relaxed text-[#d6dbe4]">{text || "(no output)"}</pre>}
      <p className="text-xs text-ink-3">Reading container logs is recorded in the audit log.</p>
    </Dialog>
  );
}

function ConsoleDialog({ endpoint, c, onClose }: { endpoint: Endpoint; c: ContainerInfo; onClose: () => void }) {
  const [command, setCommand] = useState("");
  const [started, setStarted] = useState(false);
  return (
    <Dialog open onClose={onClose} wide title={`Console: ${c.name}`}>
      {!started ? (
        <div className="space-y-3">
          <Field label="Shell" hint="Auto tries bash, then sh. Minimal images may only have sh.">
            <Select value={command} onChange={(e) => setCommand(e.target.value)}>
              <option value="">Auto (bash or sh)</option>
              {["sh", "bash", "ash", "zsh", "powershell", "pwsh", "cmd"].map((s) => <option key={s} value={s}>{s}</option>)}
            </Select>
          </Field>
          <Button variant="primary" icon={<SquareTerminal className="size-4" />} onClick={() => setStarted(true)}>Connect</Button>
        </div>
      ) : <RemoteShell endpoint={endpoint} container={{ id: c.id, name: c.name, command }} autoConnect />}
    </Dialog>
  );
}

function PullDialog({ open, onClose, onPull }: { open: boolean; onClose: () => void; onPull: (image: string) => void }) {
  const [image, setImage] = useState("");
  const go = () => { if (image.trim()) { onPull(image.trim()); setImage(""); onClose(); } };
  return (
    <Dialog open={open} onClose={onClose} title="Pull an image"
      footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!image.trim()} onClick={go}>Pull</Button></>}>
      <Field label="Image" hint="Repository and tag, for example nginx:latest or ghcr.io/owner/app:1.2. The endpoint's registry logins are used.">
        <Input className="font-mono text-[13px]" value={image} placeholder="nginx:latest" onChange={(e) => setImage(e.target.value)} onKeyDown={(e) => e.key === "Enter" && go()} />
      </Field>
    </Dialog>
  );
}

function PortainerDialog({ current, onClose, onSaved }: { current: string; onClose: () => void; onSaved: () => void }) {
  const toast = useToast();
  const [url, setUrl] = useState(current);
  const [busy, setBusy] = useState(false);
  const save = async (v: string) => {
    setBusy(true);
    try { await put("/portainer", { url: v }); toast("good", v ? "Portainer linked" : "Portainer link removed"); onSaved(); onClose(); }
    catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
  };
  return (
    <Dialog open onClose={onClose} title="Link an existing Portainer"
      footer={<>{current && <Button variant="ghost" onClick={() => void save("")}>Remove link</Button>}<Button onClick={onClose}>Cancel</Button><Button variant="primary" loading={busy} onClick={() => void save(url.trim())}>Save</Button></>}>
      <p className="text-sm text-ink-2">The built-in container tools cover day-to-day work. If you already run Portainer for stacks, registries and RBAC there, link it and an "Open Portainer" button appears on every endpoint's Containers tab.</p>
      <Field label="Portainer address"><Input value={url} placeholder="https://portainer.corp.example:9443" onChange={(e) => setUrl(e.target.value)} /></Field>
    </Dialog>
  );
}
