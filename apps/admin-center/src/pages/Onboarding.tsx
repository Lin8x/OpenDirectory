import { useEffect, useState, type ReactNode } from "react";
import { ArrowLeft, ArrowRight, Building2, Check, KeyRound, Laptop, ListChecks, Network, Server, ShieldCheck, SlidersHorizontal, WifiOff } from "lucide-react";
import { BaselineStep, ModifiersStep, ReviewStep, useBaselineCatalog, type BaselineId, type Modifiers } from "../components/Baselines";
import { ApiError, desktop, isDesktop, post } from "../lib/api";
import { useSession } from "../lib/session";
import { setAutostart } from "../lib/autostart";
import { JoinDialog } from "./Replication";
import { BrandMark, useBranding } from "../lib/branding";
import { Button, Callout, Field, Input, Select, Switch, cx } from "../components/ui";

export function Frame({ children, wide }: { children: ReactNode; wide?: boolean }) {
  return (
    <div className="relative grid min-h-full place-items-center overflow-hidden px-6 py-10">
      <div className="pointer-events-none absolute -left-40 -top-40 size-[520px] rounded-full bg-accent/10 blur-3xl" />
      <div className="pointer-events-none absolute -bottom-48 -right-40 size-[560px] rounded-full bg-[#7c4dff]/10 blur-3xl" />
      <div className={cx("anim-pop relative w-full", wide ? "max-w-3xl" : "max-w-md")}>
        <div className="mb-6 flex items-center justify-center gap-3"><BrandMark className="size-10 max-w-40" /></div>
        {children}
      </div>
    </div>
  );
}

// -------------------------------------------------------------- connection (desktop)
export function ConnectPage() {
  const { setConnection } = useSession();
  const [mode, setMode] = useState<"pick" | "remote">("pick");
  const [url, setUrl] = useState("https://");
  const [fp, setFp] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const host = async () => {
    setBusy(true); setErr(null);
    try { setConnection(await desktop.hostLocally(8443)); } catch (e) { setErr(String(e)); } finally { setBusy(false); }
  };
  const connect = async () => {
    setBusy(true); setErr(null);
    try { setConnection(await desktop.connectRemote(url.trim(), fp.trim())); } catch (e) { setErr(String(e)); } finally { setBusy(false); }
  };

  if (mode === "remote") {
    return (
      <Frame>
        <div className="rounded-2xl border border-line bg-panel p-7 shadow-xl">
          <h1 className="text-xl font-semibold">Connect to a Management Server</h1>
          <p className="mt-1 text-sm text-ink-2">Find both values in the Admin Center on the server, under Server &amp; connection.</p>
          <div className="mt-5 space-y-4">
            <Field label="Server address" hint="For example https://10.0.0.4:8443 or https://dc01.corp.example.com:8443"><Input value={url} onChange={(e) => setUrl(e.target.value)} autoFocus /></Field>
            <Field label="TLS fingerprint" term="fingerprint" hint="Starts with sha256: and proves you're talking to your own server."><Input value={fp} onChange={(e) => setFp(e.target.value)} placeholder="sha256:…" className="font-mono text-xs" /></Field>
            {err && <Callout tone="warn">{err}</Callout>}
            <div className="flex justify-between pt-2">
              <Button variant="ghost" icon={<ArrowLeft className="size-4" />} onClick={() => setMode("pick")}>Back</Button>
              <Button variant="primary" loading={busy} onClick={connect} disabled={!url.startsWith("http")}>Connect</Button>
            </div>
          </div>
        </div>
      </Frame>
    );
  }

  return (
    <Frame wide>
      <div className="text-center">
        <h1 className="text-2xl font-semibold tracking-tight">Welcome to OpenDirectory</h1>
        <p className="mx-auto mt-2 max-w-xl text-sm text-ink-2">Directory, Group Policy and endpoint management for Windows, macOS and Linux. Everything runs on your own network, with no internet or cloud account required.</p>
      </div>
      <div className="mt-8 grid gap-4 sm:grid-cols-2">
        <Choice icon={<Server className="size-5" />} title="Host the directory on this computer" onClick={host} busy={busy}
          body="This computer becomes your Management Server. Endpoints on your network connect to it. Choose this for your first or only server." badge="Recommended" />
        <Choice icon={<Network className="size-5" />} title="Connect to an existing server" onClick={() => setMode("remote")}
          body="Manage a Management Server running on another computer, for example from your own workstation." />
      </div>
      {err && <div className="mt-4"><Callout tone="warn">{err}</Callout></div>}
    </Frame>
  );
}

function Choice({ icon, title, body, onClick, badge, busy }: { icon: ReactNode; title: string; body: string; onClick: () => void; badge?: string; busy?: boolean }) {
  return (
    <button onClick={onClick} disabled={busy} className="group rounded-2xl border border-line bg-panel p-6 text-left shadow-sm transition hover:-translate-y-0.5 hover:border-accent hover:shadow-lg disabled:opacity-60">
      <div className="flex items-center justify-between">
        <div className="grid size-10 place-items-center rounded-xl bg-accent-soft text-accent">{icon}</div>
        {badge && <span className="rounded-full bg-accent-soft px-2 py-0.5 text-xs font-medium text-accent">{badge}</span>}
      </div>
      <h3 className="mt-4 font-semibold">{title}</h3>
      <p className="mt-1.5 text-sm leading-relaxed text-ink-2">{body}</p>
      <div className="mt-4 flex items-center gap-1 text-sm font-medium text-accent">{busy ? "Starting…" : "Continue"}<ArrowRight className="size-4 transition group-hover:translate-x-0.5" /></div>
    </button>
  );
}

export function UnreachablePage() {
  const { error, refresh, connection, setConnection } = useSession();
  return (
    <Frame>
      <div className="rounded-2xl border border-line bg-panel p-7 text-center shadow-xl">
        <div className="mx-auto grid size-12 place-items-center rounded-xl bg-warn-soft text-warn"><WifiOff className="size-6" /></div>
        <h1 className="mt-4 text-lg font-semibold">Can't reach the Management Server</h1>
        <p className="mt-1 text-sm text-ink-2">{connection?.url ?? "The server"} isn't answering.</p>
        {error && <p className="selectable mt-3 rounded-md bg-panel-2 px-3 py-2 text-left font-mono text-xs text-ink-2">{error}</p>}
        <div className="mt-5 flex justify-center gap-2">
          {isDesktop && <Button onClick={async () => setConnection(await desktop.disconnect())}>Change server</Button>}
          <Button variant="primary" onClick={() => void refresh()}>Try again</Button>
        </div>
      </div>
    </Frame>
  );
}

// -------------------------------------------------------------- first-run setup
const STEPS = ["Organization", "Domain Admin", "Baseline", "Customize", "Review"] as const;

export function SetupWizard() {
  const { signIn } = useSession();
  const [step, setStep] = useState(0);
  const [org, setOrg] = useState("");
  const [domain, setDomain] = useState("");
  const [name, setName] = useState("");
  const [username, setUsername] = useState("");
  const [userTouched, setUserTouched] = useState(false);
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const catalog = useBaselineCatalog();
  const [baseline, setBaseline] = useState<BaselineId>("small_business");
  const [mods, setMods] = useState<Modifiers>({ compliance: [] });
  const [launch, setLaunch] = useState(true);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [join, setJoin] = useState(false);

  const suggestDomain = (o: string) => {
    const slug = o.toLowerCase().replace(/[^a-z0-9]+/g, "").slice(0, 20);
    return slug ? `corp.${slug}.local` : "";
  };
  const domainOk = /^([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z0-9-]{2,63}$/i.test(domain.trim());
  const userOk = /^[a-z][a-z0-9._-]{0,19}$/i.test(username.trim());
  const canNext = [org.trim() && domainOk, name.trim() && userOk && pw.length >= 8 && pw === pw2, !!catalog, true, true][step];
  const last = STEPS.length - 1;

  const finish = async () => {
    setBusy(true); setErr(null);
    try {
      const r = await post<{ token: string }>("/setup", { organization: org, domain, admin_username: username, admin_display_name: name, admin_password: pw, baseline, modifiers: mods });
      if (isDesktop && launch) await setAutostart(true).catch(() => {});
      await signIn(r.token);
    } catch (e) { setErr((e as ApiError).message); setBusy(false); }
  };

  return (
    <Frame wide>
      <JoinDialog open={join} onClose={() => setJoin(false)} onJoined={() => window.location.reload()} />
      <div className="grid overflow-hidden rounded-2xl border border-line bg-panel shadow-xl md:grid-cols-[230px_1fr]">
        <div className="border-b border-line bg-panel-2 p-6 md:border-b-0 md:border-r">
          <div className="text-xs font-semibold uppercase tracking-wider text-ink-3">Set up your domain</div>
          <ol className="mt-4 space-y-3">
            {STEPS.map((s, i) => (
              <li key={s} className="flex items-center gap-2.5 text-sm">
                <span className={cx("grid size-6 place-items-center rounded-full text-xs font-semibold", i < step ? "bg-good text-white" : i === step ? "bg-accent text-accent-ink" : "bg-line text-ink-3")}>{i < step ? <Check className="size-3.5" /> : i + 1}</span>
                <span className={i === step ? "font-medium" : "text-ink-2"}>{s}</span>
              </li>
            ))}
          </ol>
          <p className="mt-8 text-xs leading-relaxed text-ink-3">You can change everything here later. Nothing leaves this network.</p>
          <button onClick={() => setJoin(true)} className="mt-6 text-left text-xs font-medium text-accent hover:underline">Join an existing domain as a replica instead</button>
        </div>
        <div className="p-7">
          {step === 0 && (
            <div className="space-y-4">
              <Header icon={<Building2 className="size-5" />} title="Name your organization" body="The domain is the name of your directory, like an Active Directory domain. Use a name you own or one ending in .local." />
              <Field label="Organization name"><Input value={org} autoFocus placeholder="Contoso Ltd" onChange={(e) => { setOrg(e.target.value); if (!domain || domain === suggestDomain(org)) setDomain(suggestDomain(e.target.value)); }} /></Field>
              <Field label="Domain name" term="domain" hint={domain && !domainOk ? "Use a DNS-style name such as corp.contoso.com" : "Users can sign in as DOMAIN\\username or username@domain."}>
                <Input value={domain} placeholder="corp.contoso.com" onChange={(e) => setDomain(e.target.value.toLowerCase())} />
              </Field>
            </div>
          )}
          {step === 1 && (
            <div className="space-y-4">
              <Header icon={<KeyRound className="size-5" />} title="Create the first Domain Admin" body="This account signs in to the Admin Center and has full control. You can add more admins later." />
              <Field label="Full name"><Input value={name} autoFocus placeholder="Dana Scott" onChange={(e) => { setName(e.target.value); if (!userTouched) setUsername((e.target.value.trim().split(/\s+/)[0] ?? "").toLowerCase().replace(/[^a-z0-9._-]/g, "")); }} /></Field>
              <Field label="Username" hint={username && !userOk ? "1 to 20 characters: letters, numbers, dot, dash or underscore, starting with a letter" : `Signs in as ${domain.split(".")[0]?.toUpperCase() || "DOMAIN"}\\${username || "username"}`}>
                <Input value={username} onChange={(e) => { setUserTouched(true); setUsername(e.target.value.toLowerCase()); }} />
              </Field>
              <div className="grid gap-4 sm:grid-cols-2">
                <Field label="Password" hint="At least 8 characters"><Input type="password" value={pw} onChange={(e) => setPw(e.target.value)} /></Field>
                <Field label="Confirm password" hint={pw2 && pw !== pw2 ? "Passwords don't match" : undefined}><Input type="password" value={pw2} onChange={(e) => setPw2(e.target.value)} /></Field>
              </div>
            </div>
          )}
          {step === 2 && catalog && (
            <div className="space-y-4">
              <Header icon={<ShieldCheck className="size-5" />} title="Choose a security baseline" body="A starting point of OUs, groups and GPOs, like Microsoft Security Baselines or CIS Benchmarks. Everything stays editable." />
              <BaselineStep catalog={catalog} value={baseline} onChange={(b) => { setBaseline(b); setMods({ compliance: [] }); }} />
            </div>
          )}
          {step === 3 && catalog && (
            <div className="space-y-4">
              <Header icon={<SlidersHorizontal className="size-5" />} title="Customize it" body="Adjust the baseline to fit. Anything you leave on its default follows the baseline." />
              <ModifiersStep catalog={catalog} baseline={baseline} value={mods} onChange={setMods} />
            </div>
          )}
          {step === 4 && (
            <div className="space-y-4">
              <Header icon={<ListChecks className="size-5" />} title="Review" body="This is exactly what will be created. Nothing changes on any computer until it enrolls." />
              <ReviewStep baseline={baseline} modifiers={mods} />
              {isDesktop && (
                <div className="rounded-xl border border-line p-3">
                  <Switch checked={launch} onChange={setLaunch} label={<span><b>Launch at startup</b> (recommended)</span>} />
                  <p className="mt-1 pl-11 text-xs text-ink-3">Starts the Admin Center in the tray when you sign in to this computer, so the directory it hosts is available after a restart. For a server that runs with nobody signed in, install the Management Server as a service instead.</p>
                </div>
              )}
              {err && <Callout tone="warn">{err}</Callout>}
            </div>
          )}
          <div className="mt-7 flex justify-between">
            <Button variant="ghost" icon={<ArrowLeft className="size-4" />} onClick={() => setStep((s) => s - 1)} disabled={step === 0}>Back</Button>
            {step < last
              ? <Button variant="primary" onClick={() => setStep((s) => s + 1)} disabled={!canNext}>Continue<ArrowRight className="size-4" /></Button>
              : <Button variant="primary" loading={busy} onClick={finish}>Create domain</Button>}
          </div>
        </div>
      </div>
    </Frame>
  );
}

function Header({ icon, title, body }: { icon: ReactNode; title: string; body: string }) {
  return (
    <div className="mb-2 flex gap-3">
      <div className="grid size-10 shrink-0 place-items-center rounded-xl bg-accent-soft text-accent">{icon}</div>
      <div><h2 className="text-lg font-semibold">{title}</h2><p className="mt-0.5 text-sm text-ink-2">{body}</p></div>
    </div>
  );
}

// -------------------------------------------------------------- sign in
export function LoginPage() {
  const { signIn, setup, connection, setConnection } = useSession();
  const { brand } = useBranding();
  const [changing, setChanging] = useState(false);
  const [forgot, setForgot] = useState(false);
  const [u, setU] = useState("");
  const [p, setP] = useState("");
  const [otp, setOtp] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true); setErr(null);
    try {
      const r = await post<{ token: string }>("/auth/login", { username: u, password: p, ...(otp !== null ? { otp } : {}) });
      await signIn(r.token);
    } catch (e) {
      // Accounts with two-factor sign-in get a second step.
      if (e instanceof ApiError && e.code === "mfa_required") {
        if (otp === null) { setOtp(""); setBusy(false); return; }
        setOtp("");
      }
      setErr((e as Error).message); setBusy(false);
    }
  };
  return (
    <Frame>
      <form onSubmit={submit} className="rounded-2xl border border-line bg-panel p-7 shadow-xl">
        <h1 className="text-xl font-semibold">Sign in</h1>
        <p className="mt-1 text-sm text-ink-2">{setup?.organization ?? "Your organization"} · <span className="text-ink-3">{setup?.domain}</span></p>
        <div className="mt-5 space-y-4">
          <Field label="Username"><Input value={u} onChange={(e) => setU(e.target.value)} autoFocus placeholder={`${setup?.domain?.split(".")[0]?.toUpperCase() ?? "DOMAIN"}\\username`} autoComplete="username" /></Field>
          <Field label="Password"><Input type="password" value={p} onChange={(e) => { setP(e.target.value); setOtp(null); }} autoComplete="current-password" /></Field>
          {otp !== null && (
            <Field label="Authenticator code" hint="The 6-digit code from your authenticator app.">
              <Input value={otp} onChange={(e) => setOtp(e.target.value.replace(/[^0-9]/g, "").slice(0, 6))} inputMode="numeric" autoComplete="one-time-code" autoFocus placeholder="123456" />
            </Field>
          )}
          {err && <Callout tone="warn">{err}</Callout>}
          <Button variant="primary" className="w-full" loading={busy} type="submit" disabled={!u || !p || (otp !== null && otp.length !== 6)}>{otp !== null ? "Verify" : "Sign in"}</Button>
        </div>
        <div className="mt-5 flex items-center justify-between border-t border-line pt-4 text-xs text-ink-3">
          <span className="flex items-center gap-1.5"><Laptop className="size-3.5" />{connection?.mode === "local" ? "Hosted on this computer" : connection?.url ?? "Development server"}</span>
          {isDesktop && <button type="button" className="text-accent hover:underline" onClick={async () => setConnection(await desktop.disconnect())}>Change server</button>}
        </div>
        <p className="mt-3 text-center text-xs"><button type="button" className="text-accent hover:underline" onClick={() => setChanging(true)}>Change password or use a reset code</button></p>
        {isDesktop && connection?.mode === "local" && <p className="mt-1 text-center text-xs"><button type="button" className="text-accent hover:underline" onClick={() => setForgot(true)}>Forgot password?</button></p>}
        {brand.support_contact && <p className="mt-1 text-center text-xs text-ink-3">Need help? Contact {brand.support_contact}</p>}
      </form>
      {changing && <ChangePassword initialUser={u} onClose={() => setChanging(false)} />}
      {forgot && <ForgotPassword onClose={() => setForgot(false)} onDone={(user) => { setForgot(false); setU(user); setP(""); setErr(null); }} />}
    </Frame>
  );
}

/** Break-glass recovery for a directory hosted on this computer: the OS asks
 *  for an administrator password (the app never sees it), then a Domain
 *  Admin's password is reset and the reset is written to the audit log. */
function ForgotPassword({ onClose, onDone }: { onClose: () => void; onDone: (user: string) => void }) {
  const [accounts, setAccounts] = useState<{ username: string; display_name: string; enabled: boolean }[] | null>(null);
  const [user, setUser] = useState("");
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  useEffect(() => {
    desktop.recoveryAccounts().then((a) => { setAccounts(a); setUser(a[0]?.username ?? ""); }).catch((e) => setErr(String(e)));
  }, []);
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-4" onClick={onClose}>
      <form className="w-full max-w-sm space-y-3 rounded-2xl border border-line bg-panel p-6 shadow-xl" onClick={(e) => e.stopPropagation()} onSubmit={async (e) => {
        e.preventDefault(); setBusy(true); setErr(null);
        try { await desktop.recoveryReset(user, pw); setDone(true); } catch (e) { setErr(String(e)); } finally { setBusy(false); }
      }}>
        <h2 className="flex items-center gap-2 text-lg font-semibold"><ShieldCheck className="size-5 text-accent" />Reset a Domain Admin password</h2>
        {done ? (
          <>
            <Callout tone="good">The password for {user} was reset and the reset was recorded in the audit log. Sign in with the new password.</Callout>
            <div className="flex justify-end pt-1"><Button type="button" variant="primary" onClick={() => onDone(user)}>Back to sign in</Button></div>
          </>
        ) : (
          <>
            <p className="text-sm text-ink-2">The directory is hosted on this computer, so an administrator of this computer can reset it. Your computer will ask for its administrator password in its own window; OpenDirectory never sees that password.</p>
            {accounts && accounts.length === 0 && <Callout tone="warn">This directory has no Domain Admins.</Callout>}
            {accounts && accounts.length > 0 && (
              <Field label="Account">
                <Select value={user} onChange={(e) => setUser(e.target.value)}>
                  {accounts.map((a) => <option key={a.username} value={a.username}>{a.username} ({a.display_name}){a.enabled ? "" : ", disabled"}</option>)}
                </Select>
              </Field>
            )}
            <Field label="New password"><Input type="password" value={pw} onChange={(e) => setPw(e.target.value)} autoComplete="new-password" /></Field>
            <Field label="Confirm new password" hint={pw2 && pw !== pw2 ? "Passwords don't match" : undefined}><Input type="password" value={pw2} onChange={(e) => setPw2(e.target.value)} autoComplete="new-password" /></Field>
            {err && <Callout tone="warn">{err}</Callout>}
            <div className="flex justify-end gap-2 pt-1">
              <Button type="button" onClick={onClose}>Cancel</Button>
              <Button type="submit" variant="primary" loading={busy} disabled={!user || !pw || pw !== pw2}>Reset password</Button>
            </div>
          </>
        )}
      </form>
    </div>
  );
}

/** Self-service: anyone in the directory can change their password here,
 *  with their current password or a one-time code from the help desk. */
function ChangePassword({ initialUser, onClose }: { initialUser: string; onClose: () => void }) {
  const [user, setUser] = useState(initialUser);
  const [mode, setMode] = useState<"password" | "code">("password");
  const [cur, setCur] = useState("");
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/40 p-4" onClick={onClose}>
      <form className="w-full max-w-sm space-y-3 rounded-2xl border border-line bg-panel p-6 shadow-xl" onClick={(e) => e.stopPropagation()} onSubmit={async (e) => {
        e.preventDefault(); setBusy(true); setMsg(null);
        try {
          const r = await post<{ message: string }>("/auth/change-password", { username: user, new_password: pw, ...(mode === "password" ? { current_password: cur } : { reset_code: cur }) });
          setMsg({ ok: true, text: r.message });
        } catch (err) { setMsg({ ok: false, text: (err as Error).message }); } finally { setBusy(false); }
      }}>
        <h2 className="text-lg font-semibold">Change your password</h2>
        <Field label="Username"><Input value={user} onChange={(e) => setUser(e.target.value)} autoFocus={!initialUser} /></Field>
        <div className="flex gap-1 rounded-lg bg-panel-2 p-1 text-xs">
          {(["password", "code"] as const).map((m) => <button type="button" key={m} onClick={() => { setMode(m); setCur(""); }} className={cx("flex-1 rounded-md py-1", mode === m ? "bg-panel shadow" : "text-ink-3")}>{m === "password" ? "Current password" : "Reset code"}</button>)}
        </div>
        <Field label={mode === "password" ? "Current password" : "Reset code from your IT team"}><Input type={mode === "password" ? "password" : "text"} value={cur} onChange={(e) => setCur(e.target.value)} autoFocus={!!initialUser} placeholder={mode === "code" ? "XXXXX-XXXXX" : undefined} /></Field>
        <Field label="New password"><Input type="password" value={pw} onChange={(e) => setPw(e.target.value)} /></Field>
        <Field label="Confirm new password" hint={pw2 && pw !== pw2 ? "Passwords don't match" : undefined}><Input type="password" value={pw2} onChange={(e) => setPw2(e.target.value)} /></Field>
        {msg && <Callout tone={msg.ok ? "good" : "warn"}>{msg.text}</Callout>}
        <div className="flex justify-end gap-2 pt-1">
          <Button type="button" onClick={onClose}>{msg?.ok ? "Close" : "Cancel"}</Button>
          {!msg?.ok && <Button type="submit" variant="primary" loading={busy} disabled={!user || !cur || !pw || pw !== pw2}>Change password</Button>}
        </div>
      </form>
    </div>
  );
}
