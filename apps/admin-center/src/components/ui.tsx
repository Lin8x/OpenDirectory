import {
  createContext, useCallback, useContext, useEffect, useState,
  type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes, type TextareaHTMLAttributes,
} from "react";
import { AlertTriangle, Check, CheckCircle2, Copy, Info, Lightbulb, Loader2, X, XCircle } from "lucide-react";
import type { ComplianceState, OsFamily } from "../lib/types";
import { GLOSSARY } from "../lib/glossary";
import { useLearnMode } from "../lib/session";

export function cx(...c: (string | false | null | undefined)[]) { return c.filter(Boolean).join(" "); }

// ------------------------------------------------------------ buttons & inputs
type Variant = "primary" | "secondary" | "ghost" | "danger";
export function Button({ variant = "secondary", size = "md", loading, icon, children, className, disabled, ...rest }:
  ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md"; loading?: boolean; icon?: ReactNode }) {
  const v = {
    primary: "bg-accent text-accent-ink hover:brightness-110 shadow-sm",
    secondary: "bg-panel border border-line text-ink hover:bg-panel-2",
    ghost: "text-ink-2 hover:bg-panel-2 hover:text-ink",
    danger: "bg-bad text-white hover:brightness-110 shadow-sm",
  }[variant];
  return (
    <button {...rest} disabled={disabled || loading}
      className={cx("inline-flex items-center justify-center gap-1.5 rounded-md font-medium transition disabled:opacity-50 disabled:pointer-events-none whitespace-nowrap focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-1",
        size === "sm" ? "h-7 px-2.5 text-[13px]" : "h-8 px-3 text-sm", v, className)}>
      {loading ? <Loader2 className="size-4 animate-spin" /> : icon}
      {children}
    </button>
  );
}

const fieldBase = "w-full rounded-md border border-line bg-panel px-2.5 text-sm text-ink placeholder:text-ink-3 outline-none transition focus:border-accent focus:ring-2 focus:ring-accent/20 disabled:opacity-60";
export function Input(props: InputHTMLAttributes<HTMLInputElement>) {
  return <input {...props} className={cx(fieldBase, "h-8", props.className)} />;
}
export function Textarea(props: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea {...props} className={cx(fieldBase, "py-2 leading-relaxed", props.className)} />;
}
export function Select({ children, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return <select {...props} className={cx(fieldBase, "h-8 pr-7", props.className)}>{children}</select>;
}
export function Field({ label, hint, children, term }: { label: string; hint?: ReactNode; children: ReactNode; term?: string }) {
  return (
    <label className="block space-y-1.5">
      <span className="flex items-center gap-1.5 text-[13px] font-medium text-ink-2">{label}{term && <TermTip term={term} />}</span>
      {children}
      {hint && <span className="block text-xs text-ink-3">{hint}</span>}
    </label>
  );
}
export function Switch({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label?: ReactNode; disabled?: boolean }) {
  return (
    <button type="button" role="switch" aria-checked={checked} disabled={disabled} onClick={() => onChange(!checked)}
      className="inline-flex items-center gap-2 text-sm text-ink disabled:opacity-50">
      <span className={cx("relative h-5 w-9 rounded-full transition", checked ? "bg-accent" : "bg-line")}>
        <span className={cx("absolute top-0.5 size-4 rounded-full bg-white shadow transition", checked ? "left-[18px]" : "left-0.5")} />
      </span>
      {label}
    </button>
  );
}

// ------------------------------------------------------------ surfaces
export function Card({ title, actions, children, className, padded = true, subtitle }:
  { title?: ReactNode; actions?: ReactNode; children: ReactNode; className?: string; padded?: boolean; subtitle?: ReactNode }) {
  return (
    <section className={cx("rounded-xl border border-line bg-panel shadow-[0_1px_2px_rgba(16,24,40,.04)]", className)}>
      {(title || actions) && (
        <header className="flex items-center justify-between gap-3 border-b border-line px-4 py-3">
          <div><h3 className="text-sm font-semibold">{title}</h3>{subtitle && <p className="text-xs text-ink-3 mt-0.5">{subtitle}</p>}</div>
          <div className="flex items-center gap-2">{actions}</div>
        </header>
      )}
      <div className={padded ? "p-4" : ""}>{children}</div>
    </section>
  );
}

export function PageHeader({ title, subtitle, actions, term }: { title: string; subtitle?: ReactNode; actions?: ReactNode; term?: string }) {
  return (
    <div className="mb-5 flex flex-wrap items-end justify-between gap-3">
      <div>
        <h1 className="text-[22px] font-semibold tracking-tight">{title}</h1>
        {subtitle && <p className="mt-1 text-sm text-ink-2 max-w-3xl">{subtitle}</p>}
      </div>
      <div className="flex items-center gap-2">{actions}</div>
      {term && <div className="w-full"><Explain term={term} /></div>}
    </div>
  );
}

export function Badge({ tone = "neutral", children, className }: { tone?: "neutral" | "accent" | "good" | "warn" | "bad"; children: ReactNode; className?: string }) {
  const t = {
    neutral: "bg-panel-2 text-ink-2 border-line",
    accent: "bg-accent-soft text-accent border-transparent",
    good: "bg-good-soft text-good border-transparent",
    warn: "bg-warn-soft text-warn border-transparent",
    bad: "bg-bad-soft text-bad border-transparent",
  }[tone];
  return <span className={cx("inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs font-medium whitespace-nowrap", t, className)}>{children}</span>;
}

export function StatusDot({ online }: { online: boolean }) {
  return <span className={cx("inline-block size-2 rounded-full", online ? "bg-good shadow-[0_0_0_3px_var(--good-soft)]" : "bg-ink-3/50")} />;
}

export function Spinner({ className }: { className?: string }) { return <Loader2 className={cx("size-4 animate-spin text-ink-3", className)} />; }

export function Loading({ label = "Loading…" }: { label?: string }) {
  return <div className="flex items-center justify-center gap-2 py-16 text-sm text-ink-3"><Spinner />{label}</div>;
}

export function ErrorNote({ message, onRetry }: { message: string; onRetry?: () => void }) {
  return (
    <div className="flex items-center gap-3 rounded-lg border border-bad/30 bg-bad-soft px-3 py-2.5 text-sm text-bad">
      <XCircle className="size-4 shrink-0" /><span className="flex-1">{message}</span>
      {onRetry && <Button size="sm" onClick={onRetry}>Retry</Button>}
    </div>
  );
}

export function EmptyState({ icon, title, body, action }: { icon: ReactNode; title: string; body: ReactNode; action?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center px-6 py-14 text-center">
      <div className="mb-3 grid size-12 place-items-center rounded-xl bg-accent-soft text-accent">{icon}</div>
      <h3 className="text-[15px] font-semibold">{title}</h3>
      <p className="mt-1 max-w-md text-sm text-ink-2">{body}</p>
      {action && <div className="mt-4">{action}</div>}
    </div>
  );
}

/** Learn-mode explanation box for a glossary term. Hidden when Learn mode is off. */
export function Explain({ term, children }: { term?: string; children?: ReactNode }) {
  const { learn } = useLearnMode();
  if (!learn) return null;
  const t = term ? GLOSSARY[term] : undefined;
  return (
    <div className="anim-fade flex gap-3 rounded-lg border border-accent/20 bg-accent-soft/60 px-3.5 py-3 text-[13px] leading-relaxed text-ink-2">
      <Lightbulb className="mt-0.5 size-4 shrink-0 text-accent" />
      <div>
        {t && <><span className="font-semibold text-ink">{t.term}{t.adName ? <span className="font-normal text-ink-3"> · AD: {t.adName}</span> : null}</span><br />{t.long}</>}
        {children}
      </div>
    </div>
  );
}

/** Small (i) tooltip with a term's short definition. */
export function TermTip({ term }: { term: string }) {
  const t = GLOSSARY[term];
  if (!t) return null;
  return (
    <span className="group relative inline-flex">
      <Info className="size-3.5 text-ink-3 hover:text-accent" />
      <span className="pointer-events-none absolute left-1/2 top-5 z-50 w-64 -translate-x-1/2 rounded-md border border-line bg-panel p-2.5 text-xs font-normal leading-relaxed text-ink-2 opacity-0 shadow-lg transition group-hover:opacity-100">
        <b className="text-ink">{t.term}</b> · {t.short}
      </span>
    </span>
  );
}

export function Callout({ tone = "info", title, children }: { tone?: "info" | "warn" | "good"; title?: ReactNode; children: ReactNode }) {
  const map = {
    info: ["border-accent/20 bg-accent-soft/50", <Info key="i" className="size-4 text-accent" />],
    warn: ["border-warn/30 bg-warn-soft", <AlertTriangle key="w" className="size-4 text-warn" />],
    good: ["border-good/30 bg-good-soft", <CheckCircle2 key="g" className="size-4 text-good" />],
  } as const;
  const [cls, icon] = map[tone];
  return (
    <div className={cx("flex gap-3 rounded-lg border px-3.5 py-3 text-[13px] leading-relaxed text-ink-2", cls)}>
      <span className="mt-0.5 shrink-0">{icon}</span>
      <div>{title && <div className="font-semibold text-ink">{title}</div>}{children}</div>
    </div>
  );
}

// ------------------------------------------------------------ overlays
export function Dialog({ open, onClose, title, children, footer, wide }: { open: boolean; onClose: () => void; title: ReactNode; children: ReactNode; footer?: ReactNode; wide?: boolean }) {
  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [open, onClose]);
  if (!open) return null;
  return (
    <div className="anim-fade fixed inset-0 z-50 grid place-items-center bg-black/40 p-4 backdrop-blur-[2px]" onMouseDown={onClose}>
      <div className={cx("anim-pop flex max-h-[88vh] w-full flex-col rounded-xl border border-line bg-panel shadow-2xl", wide ? "max-w-3xl" : "max-w-lg")} onMouseDown={(e) => e.stopPropagation()}>
        <header className="flex items-center justify-between border-b border-line px-5 py-3.5">
          <h2 className="text-[15px] font-semibold">{title}</h2>
          <button onClick={onClose} className="rounded p-1 text-ink-3 hover:bg-panel-2 hover:text-ink" aria-label="Close"><X className="size-4" /></button>
        </header>
        <div className="overflow-y-auto px-5 py-4 space-y-4">{children}</div>
        {footer && <footer className="flex justify-end gap-2 border-t border-line bg-panel-2/60 px-5 py-3 rounded-b-xl">{footer}</footer>}
      </div>
    </div>
  );
}

export function Drawer({ open, onClose, title, subtitle, children, actions }: { open: boolean; onClose: () => void; title: ReactNode; subtitle?: ReactNode; children: ReactNode; actions?: ReactNode }) {
  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [open, onClose]);
  if (!open) return null;
  return (
    <div className="anim-fade fixed inset-0 z-40 bg-black/25" onMouseDown={onClose}>
      <aside className="anim-slide absolute right-0 top-0 flex h-full w-full max-w-xl flex-col border-l border-line bg-panel shadow-2xl" onMouseDown={(e) => e.stopPropagation()}>
        <header className="flex items-start justify-between gap-3 border-b border-line px-5 py-4">
          <div className="min-w-0"><h2 className="truncate text-[17px] font-semibold">{title}</h2>{subtitle && <div className="mt-0.5 text-sm text-ink-3">{subtitle}</div>}</div>
          <div className="flex items-center gap-1">{actions}<button onClick={onClose} className="rounded p-1.5 text-ink-3 hover:bg-panel-2 hover:text-ink" aria-label="Close"><X className="size-4" /></button></div>
        </header>
        <div className="flex-1 overflow-y-auto px-5 py-4">{children}</div>
      </aside>
    </div>
  );
}

export function Tabs<T extends string>({ tabs, value, onChange }: { tabs: { id: T; label: ReactNode }[]; value: T; onChange: (t: T) => void }) {
  return (
    <div className="mb-4 flex gap-1 border-b border-line">
      {tabs.map((t) => (
        <button key={t.id} onClick={() => onChange(t.id)}
          className={cx("-mb-px border-b-2 px-3 py-2 text-sm font-medium transition", value === t.id ? "border-accent text-ink" : "border-transparent text-ink-3 hover:text-ink")}>
          {t.label}
        </button>
      ))}
    </div>
  );
}

// ------------------------------------------------------------ toasts & confirm
type Toast = { id: number; tone: "good" | "bad" | "info"; text: string };
const ToastCtx = createContext<(tone: Toast["tone"], text: string) => void>(() => {});
export const useToast = () => useContext(ToastCtx);
export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<Toast[]>([]);
  const push = useCallback((tone: Toast["tone"], text: string) => {
    const id = Date.now() + Math.random();
    setItems((x) => [...x, { id, tone, text }]);
    setTimeout(() => setItems((x) => x.filter((t) => t.id !== id)), tone === "bad" ? 6000 : 3500);
  }, []);
  return (
    <ToastCtx.Provider value={push}>
      {children}
      <div className="fixed bottom-4 right-4 z-[60] flex flex-col gap-2">
        {items.map((t) => (
          <div key={t.id} className="anim-pop flex max-w-sm items-start gap-2.5 rounded-lg border border-line bg-panel px-3.5 py-2.5 text-sm shadow-xl">
            {t.tone === "good" ? <CheckCircle2 className="mt-0.5 size-4 text-good" /> : t.tone === "bad" ? <XCircle className="mt-0.5 size-4 text-bad" /> : <Info className="mt-0.5 size-4 text-accent" />}
            <span>{t.text}</span>
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}

export function Confirm({ open, title, body, confirmLabel = "Confirm", danger, onConfirm, onClose }:
  { open: boolean; title: string; body: ReactNode; confirmLabel?: string; danger?: boolean; onConfirm: () => Promise<void> | void; onClose: () => void }) {
  const [busy, setBusy] = useState(false);
  return (
    <Dialog open={open} onClose={onClose} title={title}
      footer={<><Button onClick={onClose}>Cancel</Button>
        <Button variant={danger ? "danger" : "primary"} loading={busy} onClick={async () => { setBusy(true); try { await onConfirm(); onClose(); } finally { setBusy(false); } }}>{confirmLabel}</Button></>}>
      <div className="text-sm text-ink-2 leading-relaxed">{body}</div>
    </Dialog>
  );
}

export function CopyButton({ text, label }: { text: string; label?: string }) {
  const [done, setDone] = useState(false);
  return (
    <Button size="sm" variant="ghost" icon={done ? <Check className="size-3.5 text-good" /> : <Copy className="size-3.5" />}
      onClick={async () => { await navigator.clipboard.writeText(text); setDone(true); setTimeout(() => setDone(false), 1500); }}>
      {label ?? (done ? "Copied" : "Copy")}
    </Button>
  );
}

export function CodeBlock({ code, label }: { code: string; label?: string }) {
  return (
    <div className="overflow-hidden rounded-lg border border-line bg-panel-2">
      <div className="flex items-center justify-between border-b border-line px-3 py-1.5 text-xs font-medium text-ink-3">
        <span>{label}</span><CopyButton text={code} />
      </div>
      <pre className="selectable overflow-x-auto px-3 py-2.5 font-mono text-[12.5px] leading-relaxed text-ink">{code}</pre>
    </div>
  );
}

// ------------------------------------------------------------ domain visuals
export function OsIcon({ os, className }: { os: OsFamily; className?: string }) {
  const c = cx("size-4 shrink-0", className);
  if (os === "windows") return <svg viewBox="0 0 24 24" className={c} fill="#0a7cd6"><path d="M3 5.5 10.5 4.4v7.1H3zM11.5 4.3 21 3v8.5h-9.5zM3 12.5h7.5v7.1L3 18.5zM11.5 12.5H21V21l-9.5-1.3z" /></svg>;
  if (os === "macos") return <svg viewBox="0 0 24 24" className={c} fill="currentColor"><path d="M16.4 12.6c0-2.4 2-3.6 2.1-3.6-1.1-1.7-2.9-1.9-3.5-1.9-1.5-.2-2.9.9-3.7.9-.8 0-1.9-.9-3.2-.8-1.6 0-3.1 1-4 2.4-1.7 3-.4 7.4 1.2 9.8.8 1.2 1.8 2.5 3 2.4 1.2 0 1.7-.8 3.1-.8 1.5 0 1.9.8 3.2.8 1.3 0 2.2-1.2 3-2.4.9-1.4 1.3-2.7 1.3-2.8 0 0-2.5-1-2.5-4zM14 5.5c.7-.8 1.1-1.9 1-3-1 0-2.1.7-2.8 1.5-.6.7-1.2 1.8-1 2.9 1 .1 2.1-.6 2.8-1.4z" /></svg>;
  return <svg viewBox="0 0 24 24" className={c}><circle cx="12" cy="12" r="10" fill="#f5b014" /><path d="M8 16.5c1-1.5 1.3-3 1.3-4.6 0-1.8 1.2-3.4 2.7-3.4s2.7 1.6 2.7 3.4c0 1.6.3 3.1 1.3 4.6" stroke="#1d1d1d" strokeWidth="1.6" fill="none" strokeLinecap="round" /><circle cx="10.8" cy="10.8" r=".8" fill="#1d1d1d" /><circle cx="13.2" cy="10.8" r=".8" fill="#1d1d1d" /></svg>;
}

export const complianceTone: Record<ComplianceState, "good" | "accent" | "warn" | "bad" | "neutral"> = {
  compliant: "good", applied: "accent", non_compliant: "warn", failed: "bad", unsupported: "neutral",
};
export const complianceLabel: Record<ComplianceState, string> = {
  compliant: "Compliant", applied: "Applied", non_compliant: "Non-compliant", failed: "Failed", unsupported: "Unsupported",
};

export function Avatar({ name, className }: { name: string; className?: string }) {
  const hue = [...name].reduce((a, c) => a + c.charCodeAt(0), 0) % 360;
  const letters = name.split(/\s+/).filter(Boolean).slice(0, 2).map((p) => p[0]!.toUpperCase()).join("") || "?";
  return <span className={cx("grid size-8 shrink-0 place-items-center rounded-full text-xs font-semibold text-white", className)} style={{ background: `hsl(${hue} 55% 52%)` }}>{letters}</span>;
}

/** Simple sortable/filterable table shell. */
export function Table({ head, children, empty }: { head: ReactNode[]; children: ReactNode; empty?: ReactNode }) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <thead><tr className="border-b border-line text-left text-xs font-medium uppercase tracking-wide text-ink-3">
          {head.map((h, i) => <th key={i} className="px-4 py-2.5 font-medium">{h}</th>)}
        </tr></thead>
        <tbody className="divide-y divide-line">{children}</tbody>
      </table>
      {empty}
    </div>
  );
}
export function Row({ children, onClick, selected }: { children: ReactNode; onClick?: () => void; selected?: boolean }) {
  return <tr onClick={onClick} className={cx("transition", onClick && "cursor-pointer hover:bg-panel-2", selected && "bg-accent-soft/50")}>{children}</tr>;
}
export function Td({ children, className }: { children?: ReactNode; className?: string }) {
  return <td className={cx("px-4 py-2.5 align-middle", className)}>{children}</td>;
}
