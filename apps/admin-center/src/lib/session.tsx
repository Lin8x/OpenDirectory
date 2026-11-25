import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { desktop, get, getToken, isDesktop, post, setToken, setUnauthorizedHandler, type ConnectionState } from "./api";
import type { ServerInfo, User } from "./types";
import { useLocalStorage } from "./hooks";

export type Phase = "loading" | "connect" | "unreachable" | "setup" | "login" | "ready";

interface Me { user: User; organization: string | null; domain: string | null; server: ServerInfo }
interface SetupInfo { needs_setup: boolean; organization: string | null; domain: string | null; server: ServerInfo }

interface SessionCtx {
  phase: Phase;
  me: Me | null;
  setup: SetupInfo | null;
  connection: ConnectionState | null;
  error: string | null;
  refresh: () => Promise<void>;
  signIn: (token: string) => Promise<void>;
  signOut: () => Promise<void>;
  setConnection: (c: ConnectionState) => void;
}

const Ctx = createContext<SessionCtx | null>(null);
export const useSession = () => useContext(Ctx)!;

export function SessionProvider({ children }: { children: ReactNode }) {
  const [phase, setPhase] = useState<Phase>("loading");
  const [me, setMe] = useState<Me | null>(null);
  const [setup, setSetup] = useState<SetupInfo | null>(null);
  const [connection, setConn] = useState<ConnectionState | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    if (isDesktop) {
      const c = await desktop.status();
      setConn(c);
      if (!c.mode) { setPhase("connect"); return; }
      if (c.error) { setError(c.error); setPhase("unreachable"); return; }
    }
    try {
      const s = await get<SetupInfo>("/setup");
      setSetup(s);
      if (s.needs_setup) { setPhase("setup"); return; }
    } catch (e) {
      setError((e as Error).message);
      setPhase("unreachable");
      return;
    }
    if (!getToken()) { setPhase("login"); return; }
    try {
      setMe(await get<Me>("/auth/me"));
      setPhase("ready");
    } catch {
      setToken(null);
      setPhase("login");
    }
  }, []);

  useEffect(() => {
    setUnauthorizedHandler(() => { setToken(null); setMe(null); setPhase("login"); });
    void refresh();
  }, [refresh]);

  const signIn = useCallback(async (t: string) => { setToken(t); await refresh(); }, [refresh]);
  const signOut = useCallback(async () => {
    try { await post("/auth/logout"); } catch { /* already gone */ }
    setToken(null); setMe(null); setPhase("login");
  }, []);
  const setConnection = useCallback((c: ConnectionState) => { setConn(c); setToken(null); void refresh(); }, [refresh]);

  return <Ctx.Provider value={{ phase, me, setup, connection, error, refresh, signIn, signOut, setConnection }}>{children}</Ctx.Provider>;
}

// Learn mode and theme preferences.
export type UiMode = "modern" | "classic";
interface PrefsCtx { learn: boolean; setLearn: (v: boolean) => void; theme: "light" | "dark" | "system"; setTheme: (t: "light" | "dark" | "system") => void; ui: UiMode; setUi: (u: UiMode) => void }
const Prefs = createContext<PrefsCtx>({ learn: true, setLearn: () => {}, theme: "system", setTheme: () => {}, ui: "modern", setUi: () => {} });
export const useLearnMode = () => useContext(Prefs);

export function PrefsProvider({ children }: { children: ReactNode }) {
  const [learn, setLearn] = useLocalStorage("od.learn", true);
  const [theme, setTheme] = useLocalStorage<"light" | "dark" | "system">("od.theme", "system");
  const [ui, setUi] = useLocalStorage<UiMode>("od.ui", "modern");
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => document.documentElement.classList.toggle("dark", theme === "dark" || (theme === "system" && mq.matches));
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);
  return <Prefs.Provider value={{ learn, setLearn, theme, setTheme, ui, setUi }}>{children}</Prefs.Provider>;
}
