// In-app updates for the Admin Center.
//
// On launch (desktop only) the app asks the Tauri updater whether a newer
// signed release exists on GitHub. If so it shows a prompt with "Update now"
// and "Remind me later". Automatic checks can be turned off in Settings, and
// "Check for updates" there always works.
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { Download, Sparkles } from "lucide-react";
import { isDesktop } from "./api";
import { useLocalStorage } from "./hooks";
import { Button, Dialog, useToast } from "../components/ui";

const REMIND_LATER_MS = 24 * 60 * 60 * 1000;

type UpdateHandle = Awaited<ReturnType<typeof import("@tauri-apps/plugin-updater").check>>;

export interface AvailableUpdate { version: string; current: string; notes?: string; date?: string }
type Status = "idle" | "checking" | "up-to-date" | "available" | "downloading" | "ready" | "error";

interface UpdatesCtx {
  supported: boolean;
  currentVersion: string | null;
  autoCheck: boolean;
  setAutoCheck: (v: boolean) => void;
  status: Status;
  lastChecked: number;
  error: string | null;
  available: AvailableUpdate | null;
  progress: number | null;
  checkNow: () => Promise<void>;
  install: () => Promise<void>;
  openPrompt: () => void;
}

const Ctx = createContext<UpdatesCtx | null>(null);
export const useUpdates = () => {
  const c = useContext(Ctx);
  if (!c) throw new Error("useUpdates outside UpdatesProvider");
  return c;
};

export function UpdatesProvider({ children }: { children: ReactNode }) {
  const toast = useToast();
  const [autoCheck, setAutoCheck] = useLocalStorage("od.updates.autoCheck", true);
  const [remindAfter, setRemindAfter] = useLocalStorage<number>("od.updates.remindAfter", 0);
  const [currentVersion, setCurrentVersion] = useState<string | null>(null);
  const [status, setStatus] = useState<Status>("idle");
  const [error, setError] = useState<string | null>(null);
  const [available, setAvailable] = useState<AvailableUpdate | null>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [promptOpen, setPromptOpen] = useState(false);
  const [lastChecked, setLastChecked] = useLocalStorage<number>("od.updates.lastChecked", 0);
  const handle = useRef<UpdateHandle>(null);

  useEffect(() => {
    if (!isDesktop) return;
    import("@tauri-apps/api/app").then((m) => m.getVersion()).then(setCurrentVersion).catch(() => {});
  }, []);

  const runCheck = useCallback(async (manual: boolean) => {
    if (!isDesktop) return;
    setStatus("checking");
    setError(null);
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const u = await check();
      handle.current = u;
      if (u) {
        setAvailable({ version: u.version, current: u.currentVersion, notes: u.body ?? undefined, date: u.date ?? undefined });
        setStatus("available");
        setPromptOpen(true);
      } else {
        setAvailable(null);
        setStatus("up-to-date");
        setLastChecked(Date.now());
        // Say so after the launch check too, so it's clear the check ran.
        const v = await import("@tauri-apps/api/app").then((m) => m.getVersion()).catch(() => "");
        toast("good", v ? `You're up to date (v${v}).` : "You're up to date.");
      }
    } catch (e) {
      setStatus("error");
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg);
      setLastChecked(Date.now());
      if (manual) toast("bad", friendlyUpdateError(msg));
    }
  }, [toast, setLastChecked]);

  // Check once when the app opens, unless turned off or snoozed.
  const checkedOnLaunch = useRef(false);
  useEffect(() => {
    if (!isDesktop || checkedOnLaunch.current || !autoCheck || Date.now() < remindAfter) return;
    checkedOnLaunch.current = true;
    const t = setTimeout(() => void runCheck(false), 2500);
    return () => clearTimeout(t);
  }, [autoCheck, remindAfter, runCheck]);

  const install = useCallback(async () => {
    const u = handle.current;
    if (!u) return;
    setStatus("downloading");
    setProgress(0);
    try {
      let total = 0;
      let done = 0;
      await u.downloadAndInstall((ev) => {
        if (ev.event === "Started") total = ev.data.contentLength ?? 0;
        else if (ev.event === "Progress") {
          done += ev.data.chunkLength;
          setProgress(total ? Math.min(100, Math.round((done / total) * 100)) : null);
        } else if (ev.event === "Finished") setProgress(100);
      });
      setStatus("ready");
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (e) {
      setStatus("error");
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg);
      toast("bad", `Update failed: ${msg}`);
    }
  }, [toast]);

  const remindLater = () => {
    setRemindAfter(Date.now() + REMIND_LATER_MS);
    setPromptOpen(false);
  };

  const value: UpdatesCtx = {
    supported: isDesktop,
    currentVersion,
    autoCheck,
    setAutoCheck,
    status,
    lastChecked,
    error,
    available,
    progress,
    checkNow: () => runCheck(true),
    install,
    openPrompt: () => setPromptOpen(true),
  };

  const busy = status === "downloading" || status === "ready";
  return (
    <Ctx.Provider value={value}>
      {children}
      <Dialog
        open={promptOpen && !!available}
        onClose={() => (busy ? undefined : remindLater())}
        title={<span className="flex items-center gap-2"><Sparkles className="size-4 text-accent" />Update available</span>}
        footer={
          <>
            <Button variant="ghost" disabled={busy} onClick={remindLater}>Remind me later</Button>
            <Button variant="primary" icon={<Download className="size-4" />} loading={busy} onClick={install}>
              {status === "downloading" ? (progress != null ? `Downloading… ${progress}%` : "Downloading…") : status === "ready" ? "Restarting…" : "Update now"}
            </Button>
          </>
        }
      >
        <p className="text-sm">
          OpenDirectory Admin Center <b>{available?.version}</b> is ready to install. You have {available?.current}.
        </p>
        {available?.notes && (
          <div className="selectable max-h-56 overflow-y-auto whitespace-pre-wrap rounded-lg border border-line bg-panel-2 p-3 text-xs text-ink-2">{available.notes}</div>
        )}
        <p className="text-xs text-ink-3">
          The app restarts to finish. If this computer hosts the directory, the Management Server restarts with it and endpoints reconnect on their own within a minute.
          You can turn off update checks in Settings.
        </p>
        {status === "downloading" && (
          <div className="h-1.5 overflow-hidden rounded-full bg-line">
            <div className="h-full bg-accent transition-all" style={{ width: `${progress ?? 30}%` }} />
          </div>
        )}
      </Dialog>
    </Ctx.Provider>
  );
}

/** Plain-language reason an update check failed. */
export function friendlyUpdateError(e: string | null): string {
  if (!e) return "Couldn't check for updates.";
  if (/release JSON|404|Not Found/i.test(e)) return "Couldn't tell whether you're up to date: the latest release was published without update information.";
  if (/network|dns|connect|timed? ?out|offline/i.test(e)) return "Couldn't reach GitHub to check for updates. Check your internet connection.";
  return `Couldn't check for updates: ${e}`;
}
