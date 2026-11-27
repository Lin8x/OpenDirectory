import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { CheckCircle2, Download, FileDown, Loader2, PauseCircle, PlayCircle, RefreshCw } from "lucide-react";
import { KeyRound, Plus, Trash2 } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { del, post, saveBinary } from "../lib/api";
import { useSession } from "../lib/session";
import type { Endpoint, EnrollmentToken, OrgUnit } from "../lib/types";
import { ago, dateTime } from "../lib/format";
import { Badge, Button, Callout, Card, CodeBlock, Dialog, Explain, Field, Input, OsIcon, PageHeader, Select, Tabs, useToast } from "../components/ui";
import { OuSelect, ouPath } from "./Users";

/** Agent builds this Management Server holds (Settings > Endpoint Agent updates). */
interface AgentBuilds {
  version: string;
  builds: { platform: string; cached: boolean; sha256?: string }[];
}

export default function Enrollment() {
  const { me } = useSession();
  const tokens = useQuery<EnrollmentToken[]>("/enrollment-tokens");
  const ous = useQuery<OrgUnit[]>("/ous");
  const [creating, setCreating] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [os, setOs] = useState<"windows" | "macos" | "linux" | "android">("windows");
  const gateway = useQuery<{ enabled: boolean; public_url: string; status: string }>("/internet-gateway");
  const [via, setVia] = useState<"lan" | "internet">("lan");
  const [source, setSource] = useState<"server" | "copied">("server");
  const agents = useQuery<AgentBuilds>("/agent-updates");
  const [preparing, setPreparing] = useState(false);
  // Watch for computers that enroll while this page is open.
  const [openedAt, setOpenedAt] = useState(() => Date.now());
  const [waiting, setWaiting] = useState(true);
  const endpoints = useQuery<Endpoint[]>("/endpoints", { poll: waiting ? 4000 : undefined });
  const [making, setMaking] = useState<string | null>(null);
  const [made, setMade] = useState<{ file: string; where: string | null; instructions: string } | null>(null);
  const fresh = (endpoints.data ?? []).filter((e) => new Date(e.enrolled_at).getTime() >= openedAt - 5000);
  const toast = useToast();

  const active = (tokens.data ?? []).filter((t) => (!t.expires_at || new Date(t.expires_at) > new Date()) && (t.max_uses === null || t.uses < t.max_uses));
  const tok = (tokens.data ?? []).find((t) => t.token === selected) ?? active[0];
  const gw = gateway.data?.enabled && gateway.data.public_url ? gateway.data.public_url : null;
  const url = via === "internet" && gw ? gw : me?.server.endpoint_url ?? "";
  const fp = me?.server.tls_fingerprint;
  const fpArg = fp ? ` --fingerprint ${fp}` : "";

  const sha = (platform: string) => agents.data?.builds.find((b) => b.platform === platform && b.cached)?.sha256;
  const missing = (agents.data?.builds ?? []).filter((b) => !b.cached).map((b) => b.platform);

  const cmds = useMemo(() => {
    const t = tok?.token ?? "<TOKEN>";
    const enroll = `enroll --server ${url} --token ${t}${fpArg} --install`;
    const android = `1. On the phone, open this address in the browser and install the app\n   (it comes from this Management Server; allow installing from the browser when asked):\n   ${url}/api/v1/agent/downloads/android-agent.apk\n2. Open OpenDirectory Agent and enter:\n   Server:      ${url}\n   Token:       ${t}\n   Fingerprint: ${fp ?? "(none)"}\n3. Tap Enroll. It checks in about every 15 minutes, only on a network, so it barely touches the battery.`;
    // The agent comes from this Management Server. Its certificate is
    // self-signed (the agent pins it with --fingerprint), so the download skips
    // the CA check and the SHA-256 of the build is verified instead.
    const dl = (platform: string) => `${url}/api/v1/agent/downloads/${platform}?token=${t}`;
    const shaOf = (platform: string) => sha(platform) ?? "<SHA-256>";
    if (source === "copied") {
      return { android,
        windows: `# In an elevated PowerShell (Run as Administrator), from the folder with opendirectory-agent.exe:\n.\\opendirectory-agent.exe ${enroll}`,
        macos: `# In Terminal, from the folder with opendirectory-agent:\nchmod +x opendirectory-agent\nsudo ./opendirectory-agent ${enroll}`,
        linux: `# As root, from the folder with opendirectory-agent:\nchmod +x opendirectory-agent\nsudo ./opendirectory-agent ${enroll}`,
      };
    }
    return { android,
      windows: [
        "# In an elevated PowerShell (Run as Administrator):",
        "[Net.ServicePointManager]::SecurityProtocol = 'Tls12'",
        "[Net.ServicePointManager]::ServerCertificateValidationCallback = { $true }",
        `Invoke-WebRequest -UseBasicParsing "${dl("windows-x64")}" -OutFile opendirectory-agent.exe`,
        `if ((Get-FileHash opendirectory-agent.exe).Hash -ne "${shaOf("windows-x64")}") { throw "Checksum mismatch: don't run this file" }`,
        `.\\opendirectory-agent.exe ${enroll}`,
      ].join("\n"),
      macos: [
        "# In Terminal:",
        `arch=$([ "$(uname -m)" = arm64 ] && echo arm64 || echo x64)`,
        `curl -fksSLo opendirectory-agent "${url}/api/v1/agent/downloads/macos-$arch?token=${t}"`,
        `[ "$arch" = arm64 ] && sum=${shaOf("macos-arm64")} || sum=${shaOf("macos-x64")}`,
        `echo "$sum  opendirectory-agent" | shasum -a 256 -c - || exit 1`,
        "chmod +x opendirectory-agent",
        `sudo ./opendirectory-agent ${enroll}`,
      ].join("\n"),
      linux: [
        "# In a terminal:",
        `curl -fksSLo opendirectory-agent "${dl("linux-x64")}"`,
        `echo "${shaOf("linux-x64")}  opendirectory-agent" | sha256sum -c - || exit 1`,
        "chmod +x opendirectory-agent",
        `sudo ./opendirectory-agent ${enroll}`,
      ].join("\n"),
    };
  }, [tok, url, fpArg, fp, source, agents.data]);

  const makeFile = async (platform: "windows" | "macos" | "linux" | "conf") => {
    if (!tok) return;
    setMaking(platform); setMade(null);
    try {
      const res = await post<{ file_name: string; data: string; instructions: string }>(`/enrollment-tokens/${tok.token}/file`, { platform, server: url });
      const where = await saveBinary(res.file_name, res.data);
      setMade({ file: res.file_name, where, instructions: res.instructions });
      if (!waiting) { setOpenedAt(Date.now()); setWaiting(true); }
    } catch (e) { toast("bad", (e as Error).message); } finally { setMaking(null); }
  };

  return (
    <>
      <PageHeader title="Enrollment" guide="04-endpoints-and-agents#enrollment-how-a-computer-joins" term="enrollment" subtitle="Bring a computer under management in three steps: create a token, run one command on the computer, and watch it check in." />
      <div className="grid gap-5 lg:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)]">
        <div className="space-y-5">
          <Card title="1. Enrollment token" actions={<Button size="sm" variant="primary" icon={<Plus className="size-3.5" />} onClick={() => setCreating(true)}>New token</Button>}>
            {active.length === 0 ? (
              <div className="flex items-center justify-between gap-4">
                <p className="text-sm text-ink-2">You need an enrollment token. It lets computers join and can place them in an OU automatically.</p>
                <Button variant="primary" onClick={() => setCreating(true)}>Create token</Button>
              </div>
            ) : (
              <Field label="Use token">
                <Select value={tok?.token} onChange={(e) => setSelected(e.target.value)}>
                  {active.map((t) => <option key={t.token} value={t.token}>{t.description || "Untitled"} · {t.token}</option>)}
                </Select>
              </Field>
            )}
          </Card>
          <Card title="2. Install the Endpoint Agent">
            <div className="mb-3 flex flex-wrap items-center gap-2">
              <Button size="sm" variant={source === "server" ? "primary" : "ghost"} onClick={() => setSource("server")}>Download from this server</Button>
              <Button size="sm" variant={source === "copied" ? "primary" : "ghost"} onClick={() => setSource("copied")}>I copied the file (offline)</Button>
              {gw && <span className="ml-auto flex items-center gap-2">
                <span className="text-xs text-ink-3">Connect over</span>
                <Button size="sm" variant={via === "lan" ? "primary" : "ghost"} onClick={() => setVia("lan")}>Office network</Button>
                <Button size="sm" variant={via === "internet" ? "primary" : "ghost"} onClick={() => setVia("internet")}>Internet Gateway</Button>
              </span>}
            </div>
            <Tabs value={os} onChange={setOs} tabs={[
              { id: "windows", label: <span className="flex items-center gap-1.5"><OsIcon os="windows" className="size-3.5" />Windows</span> },
              { id: "macos", label: <span className="flex items-center gap-1.5"><OsIcon os="macos" className="size-3.5" />macOS</span> },
              { id: "linux", label: <span className="flex items-center gap-1.5"><OsIcon os="linux" className="size-3.5" />Linux</span> },
              { id: "android", label: <span className="flex items-center gap-1.5"><OsIcon os="android" className="size-3.5" />Android</span> },
            ]} />
            <CodeBlock code={cmds[os]} label={os === "windows" ? "PowerShell (Administrator)" : os === "android" ? "On the phone" : "Terminal"} />
            <div className="mt-3 space-y-2 text-sm text-ink-2">
              {source === "server" && <p>The computer downloads the Endpoint Agent from this Management Server, checks its SHA-256 and enrolls. It needs to reach <code className="font-mono text-xs">{url}</code>, nothing else.</p>}
              {source === "server" && missing.length > 0 && (
                <Callout tone="warn" title="This server doesn't have every agent build yet">
                  <p>Missing: {missing.join(", ")}. The server gets the builds that match its version once, then keeps them for enrollment and agent updates.</p>
                  <Button size="sm" className="mt-2" loading={preparing} icon={<RefreshCw className="size-3.5" />} onClick={async () => {
                    setPreparing(true);
                    try {
                      const res = await post<{ platform: string; ok: boolean; error?: string }[]>("/agent-updates/fetch");
                      const bad = res.filter((x) => !x.ok);
                      toast(bad.length ? "bad" : "good", bad.length ? `Couldn't get: ${bad.map((b) => b.platform).join(", ")}. ${bad[0]?.error ?? ""}` : "Every agent build is ready");
                      void agents.reload();
                    } finally { setPreparing(false); }
                  }}>Get agent builds</Button>
                </Callout>
              )}
              {source === "copied" && <p>Copy <code className="rounded bg-panel-2 px-1 font-mono text-xs">opendirectory-agent</code> for this OS to the computer with a USB drive or file share, for example from a computer that's already enrolled. It's a single file with no dependencies.</p>}
              {os === "windows" && <p>Installs the <b>OpenDirectory Endpoint Agent</b> Windows Service (automatic start, runs as SYSTEM, restarts on failure). Works on Windows 10 and 11 Home, Pro and Enterprise, and Windows Server.</p>}
              {os === "macos" && <p>Installs a launchd daemon at <code className="font-mono text-xs">/Library/LaunchDaemons/dev.opendirectory.agent.plist</code> that starts at boot and stays running.</p>}
              {os === "android" && <p>The browser warns about the server's self-signed certificate; that's expected. The app itself pins the fingerprint above, so it only ever talks to this server.</p>}
              {os === "android" && <p>The app runs no background service: Android's job scheduler wakes it about every 15 minutes when there's a network and the battery isn't low. It reports inventory and security status, shows the managed-device notice and gives the user a Help desk chat.</p>}
              {via === "internet" && os !== "android" && <p>This command enrolls over the Internet Gateway, so the computer doesn't need to be on the office network.</p>}
              {os === "linux" && <p>Installs a systemd service <code className="font-mono text-xs">opendirectory-agent.service</code>, enabled at boot with automatic restart.</p>}
            </div>
          </Card>
          <Card title="Or hand out an enrollment file" actions={<FileDown className="size-4 text-ink-3" />}>
            <p className="mb-3 text-sm text-ink-2">No command needed: this Management Server builds a ready-to-run installer with its address, the token above and its fingerprint built in. Copy it to the computer by USB, file share or email.</p>
            <div className="flex flex-wrap gap-2">
              {([
                ["windows", "windows", "Windows (.zip)"],
                ["macos", "macos", "macOS (.command)"],
                ["linux", "linux", "Linux (.sh)"],
              ] as const).map(([platform, icon, label]) => (
                <Button key={platform} size="sm" loading={making === platform} disabled={!tok || !!making} icon={<OsIcon os={icon} className="size-3.5" />} onClick={() => makeFile(platform)}>{label}</Button>
              ))}
              <Button size="sm" variant="ghost" loading={making === "conf"} disabled={!tok || !!making} icon={<Download className="size-3.5" />} onClick={() => makeFile("conf")}>enroll.conf for fleet packages</Button>
            </div>
            {made && <div className="mt-3"><Callout tone="good" title={made.where ? `Saved ${made.file} to ${made.where}` : `Downloaded ${made.file}`}>{made.instructions}</Callout></div>}
            {!tok && <p className="mt-2 text-xs text-ink-3">Create a token first.</p>}
          </Card>
          <Card title="3. Confirm it checked in" actions={waiting
            ? <Button size="sm" variant="ghost" icon={<PauseCircle className="size-3.5" />} onClick={() => setWaiting(false)}>Stop waiting</Button>
            : <Button size="sm" variant="ghost" icon={<PlayCircle className="size-3.5" />} onClick={() => { setOpenedAt(Date.now()); setWaiting(true); }}>Wait again</Button>}>
            {!waiting && fresh.length === 0 ? (
              <div className="space-y-2 text-sm text-ink-2">
                <p>Stopped waiting. Computers can still enroll with this token; they'll appear under <Link to="/endpoints" className="text-accent hover:underline">Endpoints</Link>.</p>
                {tok && <p>To stop new enrollments with it, <button className="text-bad hover:underline" onClick={async () => { await del(`/enrollment-tokens/${tok.token}`); toast("good", "Token revoked"); void tokens.reload(); }}>revoke the token "{tok.description || tok.token}"</button>.</p>}
              </div>
            ) : fresh.length === 0 ? (
              <div className="flex items-center gap-3 text-sm text-ink-2">
                <Loader2 className="size-4 animate-spin text-accent" />
                Waiting for a computer to enroll. It shows up here within seconds of running the command or file.
              </div>
            ) : (
              <ul className="space-y-2">
                {fresh.map((e) => (
                  <li key={e.id} className="flex items-center gap-3 text-sm">
                    <CheckCircle2 className="size-4 text-good" />
                    <OsIcon os={e.os} className="size-4" />
                    <Link to={`/endpoints/${e.id}`} className="font-medium text-accent hover:underline">{e.hostname}</Link>
                    <span className="text-ink-3">enrolled {ago(e.enrolled_at)}</span>
                  </li>
                ))}
              </ul>
            )}
          </Card>
        </div>
        <div className="space-y-5">
          <Card title="This Management Server">
            <dl className="space-y-3 text-sm">
              <div><dt className="text-xs text-ink-3">Address for endpoints</dt><dd className="selectable font-mono text-[13px]">{url}</dd></div>
              {fp && <div><dt className="flex items-center gap-1 text-xs text-ink-3">TLS fingerprint</dt><dd className="selectable break-all font-mono text-[11.5px] text-ink-2">{fp}</dd></div>}
            </dl>
            <div className="mt-3"><Explain term="fingerprint" /></div>
          </Card>
          <Callout tone="info" title="Windows Home is fully supported">Home editions can't join an Active Directory domain, but the Endpoint Agent manages them like any other endpoint: sign-in accounts, GPOs, inventory and scripts.</Callout>
          <Card title="Tokens" padded={false}>
            <ul className="divide-y divide-line">
              {(tokens.data ?? []).length === 0 && <li className="px-4 py-6 text-center text-sm text-ink-3">No tokens yet</li>}
              {(tokens.data ?? []).map((t) => {
                const expired = t.expires_at && new Date(t.expires_at) < new Date();
                const used = t.max_uses !== null && t.uses >= t.max_uses;
                return (
                  <li key={t.token} className="flex items-center gap-3 px-4 py-2.5">
                    <KeyRound className="size-4 text-ink-3" />
                    <div className="min-w-0 flex-1">
                      <div className="text-sm font-medium">{t.description || "Untitled"}</div>
                      <div className="font-mono text-xs text-ink-3">{t.token}</div>
                      <div className="text-xs text-ink-3">{t.uses}{t.max_uses ? `/${t.max_uses}` : ""} used · {t.expires_at ? `expires ${dateTime(t.expires_at)}` : "never expires"}{t.ou_id && ` · → ${ouPath(ous.data ?? [], t.ou_id)}`} · {ago(t.created_at)}</div>
                    </div>
                    {expired ? <Badge>Expired</Badge> : used ? <Badge>Used up</Badge> : <Badge tone="good">Active</Badge>}
                    <Button size="sm" variant="ghost" icon={<Trash2 className="size-3.5" />} onClick={async () => { await del(`/enrollment-tokens/${t.token}`); toast("good", "Token revoked"); void tokens.reload(); }} aria-label="Revoke" />
                  </li>
                );
              })}
            </ul>
          </Card>
        </div>
      </div>
      <NewToken open={creating} ous={ous.data ?? []} onClose={() => setCreating(false)} onCreated={(t) => { void tokens.reload(); setSelected(t.token); setCreating(false); }} />
    </>
  );
}

function NewToken({ open, ous, onClose, onCreated }: { open: boolean; ous: OrgUnit[]; onClose: () => void; onCreated: (t: EnrollmentToken) => void }) {
  const toast = useToast();
  const [desc, setDesc] = useState("");
  const [ou, setOu] = useState<string | null>(null);
  const [uses, setUses] = useState("");
  const [hours, setHours] = useState("168");
  const [busy, setBusy] = useState(false);
  return (
    <Dialog open={open} onClose={onClose} title="New enrollment token"
      footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" loading={busy} onClick={async () => {
        setBusy(true);
        try { const t = await post<EnrollmentToken>("/enrollment-tokens", { description: desc, ou_id: ou, max_uses: uses ? Number(uses) : null, expires_in_hours: Number(hours) }); toast("good", "Token created"); onCreated(t); setDesc(""); setUses(""); }
        catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
      }}>Create token</Button></>}>
      <Field label="Description" hint="Where or what it's for."><Input autoFocus value={desc} onChange={(e) => setDesc(e.target.value)} placeholder="Front office laptops" /></Field>
      <Field label="Place new endpoints in" term="ou"><OuSelect ous={ous} value={ou} onChange={setOu} /></Field>
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label="Maximum uses" hint="Empty = unlimited"><Input type="number" min={1} value={uses} onChange={(e) => setUses(e.target.value)} placeholder="Unlimited" /></Field>
        <Field label="Expires after">
          <Select value={hours} onChange={(e) => setHours(e.target.value)}>
            <option value="1">1 hour</option><option value="24">1 day</option><option value="168">7 days</option><option value="720">30 days</option><option value="0">Never</option>
          </Select>
        </Field>
      </div>
    </Dialog>
  );
}
