// Mobile Devices: inventory of phones and tablets, and Apple configuration
// profiles that install without an MDM server.
import { useState } from "react";
import { FileCog, Plus, Smartphone, Trash2, Upload } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { del, post, put, saveExport } from "../lib/api";
import { ago } from "../lib/format";
import type { User } from "../lib/types";
import { Badge, Button, Callout, Card, Dialog, EmptyState, Explain, Field, Input, Loading, PageHeader, Row, Select, Switch, Table, Td, Textarea, useToast } from "../components/ui";
import { ipKey, ListToolbar, NoMatches, useListView } from "../components/DataView";

interface Device {
  id?: string; name: string; platform: "ios" | "ipados" | "android" | "other"; model: string; serial: string; imei: string; phone_number: string;
  owner_user_id: string | null; ownership: "corporate" | "byod"; ip: string; mac: string; os_version: string; status: "active" | "lost" | "retired"; notes: string; updated_at?: string | null;
}
const EMPTY: Device = { name: "", platform: "ios", model: "", serial: "", imei: "", phone_number: "", owner_user_id: null, ownership: "corporate", ip: "", mac: "", os_version: "", status: "active", notes: "" };
const PLATFORM = { ios: "iPhone (iOS)", ipados: "iPad (iPadOS)", android: "Android", other: "Other" } as const;

export default function Mobile() {
  const list = useQuery<Device[]>("/mobile-devices");
  const users = useQuery<User[]>("/users");
  const [edit, setEdit] = useState<Device | null>(null);
  const [importing, setImporting] = useState(false);
  const [profile, setProfile] = useState(false);
  const owner = (id: string | null) => users.data?.find((u) => u.id === id)?.display_name ?? "—";
  const STATUS = { active: "Active", lost: "Lost", retired: "Retired" } as const;
  const view = useListView(list.data, {
    id: "mobile",
    search: (d) => [d.name, d.serial, d.imei, d.phone_number, d.ip, d.mac, d.model, d.os_version, owner(d.owner_user_id), d.notes],
    filters: [
      { kind: "enum", key: "platform", label: "Platform", get: (d) => d.platform, options: (Object.keys(PLATFORM) as Device["platform"][]).map((k) => ({ value: k, label: PLATFORM[k] })) },
      { kind: "enum", key: "status", label: "Status", get: (d) => d.status, options: (Object.keys(STATUS) as Device["status"][]).map((k) => ({ value: k, label: STATUS[k] })) },
      { kind: "enum", key: "ownership", label: "Ownership", get: (d) => d.ownership, options: [{ value: "corporate", label: "Company-owned" }, { value: "byod", label: "Personal (BYOD)" }] },
      { kind: "enum", key: "owner", label: "Owner", get: (d) => (d.owner_user_id ? owner(d.owner_user_id) : "Nobody") },
      { kind: "date", key: "updated", label: "Updated", get: (d) => d.updated_at },
    ],
    sorts: {
      name: { label: "Device", get: (d) => d.name },
      owner: { label: "Owner", get: (d) => (d.owner_user_id ? owner(d.owner_user_id) : null) },
      serial: { label: "Serial / IMEI", get: (d) => d.serial || d.imei },
      number: { label: "Number", get: (d) => d.phone_number },
      ip: { label: "IP", get: (d) => ipKey(d.ip) },
      status: { label: "Status", get: (d) => STATUS[d.status] },
      updated: { label: "Updated", get: (d) => d.updated_at, type: "date" },
    },
    defaultSort: { key: "name", dir: "asc" },
  });
  const shown = view.rows;
  return (
    <>
      <PageHeader title="Mobile Devices" term="mdm" subtitle="Track phones and tablets by serial number, IMEI, owner and IP, and build Apple configuration profiles for passcodes, Wi-Fi, certificates and restrictions."
        actions={<div className="flex gap-2"><Button icon={<FileCog className="size-4" />} onClick={() => setProfile(true)}>Apple profile…</Button><Button icon={<Upload className="size-4" />} onClick={() => setImporting(true)}>Import CSV</Button><Button variant="primary" icon={<Plus className="size-4" />} onClick={() => setEdit({ ...EMPTY })}>Add device</Button></div>} />
      <Card padded={false}>
        {(list.data ?? []).length > 0 && <ListToolbar view={view} placeholder="Search name, serial, IMEI, number, IP or owner" noun={["device", "devices"]} />}
        {list.loading && !list.data ? <Loading /> : view.total === 0 ? <EmptyState icon={<Smartphone className="size-5" />} title="No mobile devices yet" body="Add phones and tablets one by one or import a CSV from your carrier or asset list." /> : (
          <Table view={view} head={[{ label: "Device", sort: "name" }, { label: "Owner", sort: "owner" }, { label: "Serial / IMEI", sort: "serial" }, { label: "Number", sort: "number" }, { label: "IP", sort: "ip" }, { label: "Status", sort: "status" }]}
            empty={shown.length === 0 && <NoMatches view={view} />}>
            {shown.map((d) => (
              <Row key={d.id} onClick={() => setEdit(d)}>
                <Td><div className="font-medium">{d.name}</div><div className="text-xs text-ink-3">{PLATFORM[d.platform]}{d.model ? ` · ${d.model}` : ""}{d.os_version ? ` · ${d.os_version}` : ""}</div></Td>
                <Td>{owner(d.owner_user_id)}<div className="text-xs text-ink-3">{d.ownership === "byod" ? "Personal (BYOD)" : "Company-owned"}</div></Td>
                <Td className="font-mono text-xs">{d.serial || "—"}<div className="text-ink-3">{d.imei || ""}</div></Td>
                <Td>{d.phone_number || "—"}</Td>
                <Td className="font-mono text-xs">{d.ip || "—"}</Td>
                <Td>{d.status === "lost" ? <Badge tone="bad">Lost</Badge> : d.status === "retired" ? <Badge>Retired</Badge> : <Badge tone="good">Active</Badge>}<div className="text-xs text-ink-3">{d.updated_at ? ago(d.updated_at) : ""}</div></Td>
              </Row>
            ))}
          </Table>
        )}
      </Card>
      <div className="mt-5"><Explain>Full mobile device management (remote lock and wipe, app installs, automatic enrollment) needs an Apple Push certificate from Apple Business Manager and Android Enterprise registration with Google; that's planned. Configuration profiles work today: install them by opening the file on the device.</Explain></div>
      {edit && <DeviceDialog d={edit} users={users.data ?? []} onClose={() => setEdit(null)} onSaved={() => { setEdit(null); void list.reload(); }} />}
      {importing && <ImportDialog users={users.data ?? []} onClose={() => setImporting(false)} onDone={() => { setImporting(false); void list.reload(); }} />}
      {profile && <ProfileDialog onClose={() => setProfile(false)} />}
    </>
  );
}

function DeviceDialog({ d: initial, users, onClose, onSaved }: { d: Device; users: User[]; onClose: () => void; onSaved: () => void }) {
  const toast = useToast();
  const [d, setD] = useState<Device>(initial);
  const f = (k: keyof Device) => (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement>) => setD({ ...d, [k]: e.target.value });
  const save = async () => {
    try { if (d.id) await put(`/mobile-devices/${d.id}`, d); else await post("/mobile-devices", [d]); toast("good", "Saved"); onSaved(); } catch (e) { toast("bad", (e as Error).message); }
  };
  return (
    <Dialog open onClose={onClose} wide title={d.id ? d.name : "Add a mobile device"}
      footer={<>{d.id && <Button variant="ghost" icon={<Trash2 className="size-4" />} onClick={async () => { await del(`/mobile-devices/${d.id}`); onSaved(); }}>Delete</Button>}<div className="flex-1" /><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!d.name.trim()} onClick={save}>Save</Button></>}>
      {d.status === "lost" && <Callout tone="warn" title="Marked as lost">Use Find My (Apple) or Find My Device (Google) to locate or erase it, and change its owner's password.</Callout>}
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="Name"><Input value={d.name} onChange={f("name")} placeholder="Dana's iPhone" /></Field>
        <Field label="Platform"><Select value={d.platform} onChange={f("platform")}>{Object.entries(PLATFORM).map(([k, v]) => <option key={k} value={k}>{v}</option>)}</Select></Field>
        <Field label="Model"><Input value={d.model} onChange={f("model")} placeholder="iPhone 16" /></Field>
        <Field label="OS version"><Input value={d.os_version} onChange={f("os_version")} /></Field>
        <Field label="Serial number"><Input value={d.serial} onChange={f("serial")} className="font-mono" /></Field>
        <Field label="IMEI" hint="Dial *#06# on the phone to see it"><Input value={d.imei} onChange={f("imei")} className="font-mono" /></Field>
        <Field label="Phone number"><Input value={d.phone_number} onChange={f("phone_number")} /></Field>
        <Field label="Owner"><Select value={d.owner_user_id ?? ""} onChange={(e) => setD({ ...d, owner_user_id: e.target.value || null })}><option value="">Nobody</option>{users.map((u) => <option key={u.id} value={u.id}>{u.display_name}</option>)}</Select></Field>
        <Field label="Ownership"><Select value={d.ownership} onChange={f("ownership")}><option value="corporate">Company-owned</option><option value="byod">Personal (BYOD)</option></Select></Field>
        <Field label="Status"><Select value={d.status} onChange={f("status")}><option value="active">Active</option><option value="lost">Lost</option><option value="retired">Retired</option></Select></Field>
        <Field label="IP address"><Input value={d.ip} onChange={f("ip")} className="font-mono" /></Field>
        <Field label="Wi-Fi MAC"><Input value={d.mac} onChange={f("mac")} className="font-mono" /></Field>
      </div>
      <Field label="Notes"><Textarea rows={2} value={d.notes} onChange={f("notes")} /></Field>
    </Dialog>
  );
}

function ImportDialog({ users, onClose, onDone }: { users: User[]; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const [text, setText] = useState("name,platform,model,serial,imei,phone_number,owner,ip\nDana's iPhone,ios,iPhone 16,F2LXK0ABCD,353918051234567,+1 555 0100,dana,10.0.20.77");
  const rows = text.trim().split(/\r?\n/).map((l) => l.split(",").map((c) => c.trim()));
  const head = rows[0]?.map((h) => h.toLowerCase()) ?? [];
  const devices: Device[] = rows.slice(1).filter((r) => r.some(Boolean)).map((r) => {
    const g = (k: string) => r[head.indexOf(k)] ?? "";
    const o = g("owner").toLowerCase();
    return { ...EMPTY, name: g("name"), platform: (["ios", "ipados", "android"].includes(g("platform").toLowerCase()) ? g("platform").toLowerCase() : "other") as Device["platform"], model: g("model"), serial: g("serial"), imei: g("imei"), phone_number: g("phone_number"), ip: g("ip"), owner_user_id: users.find((u) => u.username === o)?.id ?? null };
  });
  return (
    <Dialog open onClose={onClose} wide title="Import mobile devices"
      footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!devices.length} onClick={async () => { try { await post("/mobile-devices", devices); toast("good", `Imported ${devices.length} device(s)`); onDone(); } catch (e) { toast("bad", (e as Error).message); } }}>Import {devices.length}</Button></>}>
      <p className="text-sm text-ink-2">Paste CSV with a header row. Columns: name, platform (ios, ipados, android), model, serial, imei, phone_number, owner (username), ip.</p>
      <Textarea rows={8} className="font-mono text-xs" value={text} onChange={(e) => setText(e.target.value)} />
      <label className="inline-flex cursor-pointer items-center gap-1.5 text-sm text-accent"><Upload className="size-4" />Load a .csv file<input type="file" accept=".csv,text/csv" className="hidden" onChange={async (e) => { const f = e.target.files?.[0]; if (f) setText(await f.text()); }} /></label>
    </Dialog>
  );
}

function ProfileDialog({ onClose }: { onClose: () => void }) {
  const toast = useToast();
  const [name, setName] = useState("Company phone settings");
  const [passcode, setPasscode] = useState(true);
  const [pc, setPc] = useState({ min_length: 6, require_alphanumeric: false, max_inactivity_minutes: 5, max_failed_attempts: 0 });
  const [wifi, setWifi] = useState<{ ssid: string; security: string; password: string; hidden: boolean }[]>([]);
  const [trust, setTrust] = useState(false);
  const [restrict, setRestrict] = useState(false);
  const [r, setR] = useState({ allow_camera: true, allow_screenshots: true, allow_icloud_backup: true, allow_app_installs: true });
  return (
    <Dialog open onClose={onClose} wide title="Apple configuration profile"
      footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" onClick={async () => {
        try {
          const p = await post<{ file_name: string; content: string }>("/mobile-profiles/apple", { name, passcode: passcode ? pc : null, wifi, trust_ca: trust, restrictions: restrict ? r : null });
          const where = await saveExport(p.file_name, p.content);
          toast("good", where ? `Saved to ${where}` : `Downloaded ${p.file_name}`); onClose();
        } catch (e) { toast("bad", (e as Error).message); }
      }}>Download profile</Button></>}>
      <div className="max-h-[60vh] space-y-4 overflow-y-auto pr-1">
        <Field label="Profile name"><Input value={name} onChange={(e) => setName(e.target.value)} /></Field>
        <div className="rounded-xl border border-line p-3">
          <Switch checked={passcode} onChange={setPasscode} label={<b>Passcode policy</b>} />
          {passcode && <div className="mt-3 grid gap-3 sm:grid-cols-3">
            <Field label="Minimum length"><Input type="number" value={pc.min_length} onChange={(e) => setPc({ ...pc, min_length: Number(e.target.value) })} /></Field>
            <Field label="Auto-lock (minutes)"><Input type="number" value={pc.max_inactivity_minutes} onChange={(e) => setPc({ ...pc, max_inactivity_minutes: Number(e.target.value) })} /></Field>
            <Field label="Erase after failures" hint="0 = never"><Input type="number" value={pc.max_failed_attempts} onChange={(e) => setPc({ ...pc, max_failed_attempts: Number(e.target.value) })} /></Field>
            <Switch checked={pc.require_alphanumeric} onChange={(v) => setPc({ ...pc, require_alphanumeric: v })} label="Letters and numbers" />
          </div>}
        </div>
        <div className="rounded-xl border border-line p-3">
          <div className="flex items-center justify-between"><b className="text-sm">Wi-Fi networks</b><Button size="sm" onClick={() => setWifi([...wifi, { ssid: "", security: "WPA2", password: "", hidden: false }])}>Add network</Button></div>
          {wifi.map((w, i) => (
            <div key={i} className="mt-2 grid grid-cols-[1fr_130px_1fr_auto] gap-2">
              <Input placeholder="SSID" value={w.ssid} onChange={(e) => setWifi(wifi.map((x, j) => (j === i ? { ...x, ssid: e.target.value } : x)))} />
              <Select value={w.security} onChange={(e) => setWifi(wifi.map((x, j) => (j === i ? { ...x, security: e.target.value } : x)))}><option>WPA2</option><option>WPA3</option><option value="Enterprise">Enterprise</option></Select>
              <Input type="password" placeholder={w.security === "Enterprise" ? "User signs in" : "Password"} disabled={w.security === "Enterprise"} value={w.password} onChange={(e) => setWifi(wifi.map((x, j) => (j === i ? { ...x, password: e.target.value } : x)))} />
              <button className="text-ink-3 hover:text-bad" onClick={() => setWifi(wifi.filter((_, j) => j !== i))} aria-label="Remove"><Trash2 className="size-4" /></button>
            </div>
          ))}
        </div>
        <Switch checked={trust} onChange={setTrust} label="Trust this domain's certificate authority" />
        <div className="rounded-xl border border-line p-3">
          <Switch checked={restrict} onChange={setRestrict} label={<b>Restrictions</b>} />
          {restrict && <div className="mt-3 grid gap-2 sm:grid-cols-2">
            <Switch checked={r.allow_camera} onChange={(v) => setR({ ...r, allow_camera: v })} label="Allow camera" />
            <Switch checked={r.allow_screenshots} onChange={(v) => setR({ ...r, allow_screenshots: v })} label="Allow screenshots" />
            <Switch checked={r.allow_icloud_backup} onChange={(v) => setR({ ...r, allow_icloud_backup: v })} label="Allow iCloud backup" />
            <Switch checked={r.allow_app_installs} onChange={(v) => setR({ ...r, allow_app_installs: v })} label="Allow installing apps" />
          </div>}
        </div>
        <Callout tone="info">The profile isn't signed, so iOS shows it as "Unverified" during install. Restrictions apply fully on supervised devices.</Callout>
      </div>
    </Dialog>
  );
}
