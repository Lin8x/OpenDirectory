// Two-factor sign-in (TOTP): enroll an authenticator app, turn it off, and
// the domain-wide requirement. Works with Microsoft Authenticator, Google
// Authenticator, 1Password, Bitwarden, Aegis and other TOTP apps.
import { useEffect, useMemo, useState } from "react";
import qrcode from "qrcode-generator";
import { ShieldCheck, Smartphone } from "lucide-react";
import { get, post, put } from "../lib/api";
import { useSession } from "../lib/session";
import { Badge, Button, Callout, Card, CopyButton, Dialog, Field, Input, Select, useToast } from "./ui";

function Qr({ text }: { text: string }) {
  const svg = useMemo(() => {
    const q = qrcode(0, "M");
    q.addData(text);
    q.make();
    return q.createSvgTag({ cellSize: 4, margin: 2, scalable: true });
  }, [text]);
  // The QR code is generated locally from our own data, never from user HTML.
  return <div className="mx-auto size-48 rounded-lg bg-white p-1" dangerouslySetInnerHTML={{ __html: svg }} />;
}

/** Scan, confirm a code, done. */
export function MfaEnroll({ onDone }: { onDone: () => void }) {
  const [setup, setSetup] = useState<{ secret: string; otpauth_uri: string } | null>(null);
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => { post<{ secret: string; otpauth_uri: string }>("/auth/mfa/setup").then(setSetup).catch((e) => setErr((e as Error).message)); }, []);
  const grouped = setup?.secret.replace(/(.{4})/g, "$1 ").trim();
  return (
    <form className="space-y-4" onSubmit={async (e) => {
      e.preventDefault(); setBusy(true); setErr(null);
      try { await post("/auth/mfa/enable", { code }); onDone(); } catch (e) { setErr((e as Error).message); } finally { setBusy(false); }
    }}>
      <ol className="list-decimal space-y-1 pl-5 text-sm text-ink-2">
        <li>Open your authenticator app (Microsoft Authenticator, Google Authenticator, 1Password, Bitwarden…) and add an account.</li>
        <li>Scan this code, or type the setup key.</li>
        <li>Enter the 6-digit code the app shows.</li>
      </ol>
      {setup ? <Qr text={setup.otpauth_uri} /> : !err && <div className="mx-auto size-48 animate-pulse rounded-lg bg-panel-2" />}
      {grouped && <div className="flex items-center justify-center gap-2"><code className="selectable rounded bg-panel-2 px-2 py-1 font-mono text-xs">{grouped}</code><CopyButton text={setup!.secret} /></div>}
      <Field label="Code from the app">
        <Input value={code} onChange={(e) => setCode(e.target.value.replace(/[^0-9]/g, "").slice(0, 6))} inputMode="numeric" autoComplete="one-time-code" placeholder="123456" autoFocus />
      </Field>
      {err && <Callout tone="warn">{err}</Callout>}
      <Button type="submit" variant="primary" className="w-full" loading={busy} disabled={code.length !== 6 || !setup}>Turn on two-factor sign-in</Button>
    </form>
  );
}

/** Shown after sign-in when the domain requires MFA and this account has none. */
export function MfaRequired() {
  const { refresh, signOut, me } = useSession();
  return (
    <div className="rounded-2xl border border-line bg-panel p-7 shadow-xl">
      <h1 className="flex items-center gap-2 text-xl font-semibold"><ShieldCheck className="size-5 text-accent" />Set up two-factor sign-in</h1>
      <p className="mt-1 mb-4 text-sm text-ink-2">Your organization requires a second step when {me?.user.display_name || "you"} sign{me ? "s" : ""} in. It takes a minute.</p>
      <MfaEnroll onDone={() => void refresh()} />
      <p className="mt-4 text-center text-xs"><button type="button" className="text-accent hover:underline" onClick={() => void signOut()}>Sign out</button></p>
    </div>
  );
}

/** Settings card: your own two-factor sign-in, and the domain policy for Domain Admins. */
export function MfaCard() {
  const { me, refresh } = useSession();
  const toast = useToast();
  const [enroll, setEnroll] = useState(false);
  const [off, setOff] = useState(false);
  const [pw, setPw] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [policy, setPolicy] = useState<string | null>(null);
  useEffect(() => { if (me?.domain_admin) get<{ require: string }>("/mfa-policy").then((p) => setPolicy(p.require)).catch(() => {}); }, [me?.domain_admin]);
  if (!me) return null;
  return (
    <Card title={<span className="flex items-center gap-2"><Smartphone className="size-4" />Two-factor sign-in</span>}
      subtitle="A code from an authenticator app, in addition to your password"
      actions={me.mfa_enabled ? <Badge tone="good">On</Badge> : <Badge>Off</Badge>}>
      <div className="space-y-4">
        {me.mfa_enabled
          ? <Button onClick={() => setOff(true)}>Turn off</Button>
          : <Button variant="primary" onClick={() => setEnroll(true)}>Set up an authenticator app</Button>}
        {me.domain_admin && policy !== null && (
          <Field label="Require two-factor sign-in for" hint="Accounts without it are asked to set it up at their next sign-in. Lost phone? Use Reset two-factor on the user's page.">
            <Select value={policy} onChange={async (e) => {
              const v = e.target.value; setPolicy(v);
              try { await put("/mfa-policy", { require: v }); toast("good", "Sign-in policy saved"); } catch (err) { toast("bad", (err as Error).message); }
            }}>
              <option value="optional">Nobody (optional for everyone)</option>
              <option value="delegated">Delegated operators</option>
              <option value="all">Every account, Domain Admins included</option>
            </Select>
          </Field>
        )}
      </div>
      <Dialog open={enroll} onClose={() => setEnroll(false)} title="Set up two-factor sign-in">
        {enroll && <MfaEnroll onDone={() => { setEnroll(false); toast("good", "Two-factor sign-in is on"); void refresh(); }} />}
      </Dialog>
      <Dialog open={off} onClose={() => setOff(false)} title="Turn off two-factor sign-in"
        footer={<><Button onClick={() => setOff(false)}>Cancel</Button><Button variant="primary" loading={busy} disabled={!pw || code.length !== 6} onClick={async () => {
          setBusy(true);
          try { await post("/auth/mfa/disable", { password: pw, code }); setOff(false); setPw(""); setCode(""); toast("good", "Two-factor sign-in is off"); void refresh(); }
          catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
        }}>Turn off</Button></>}>
        <div className="space-y-3">
          <Field label="Password"><Input type="password" value={pw} onChange={(e) => setPw(e.target.value)} autoComplete="current-password" /></Field>
          <Field label="Code from your app"><Input value={code} onChange={(e) => setCode(e.target.value.replace(/[^0-9]/g, "").slice(0, 6))} inputMode="numeric" autoComplete="one-time-code" /></Field>
        </div>
      </Dialog>
    </Card>
  );
}
