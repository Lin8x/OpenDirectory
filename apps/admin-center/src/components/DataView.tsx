// Shared list filtering and sorting for every list and table in the Admin
// Center. A page describes its search fields, filters and sort keys once with
// useListView, then renders <ListToolbar> above the list and passes the view
// to <Table> so the column headers become click-to-sort.
import { useEffect, useRef, useState, type ReactNode } from "react";
import { ArrowDown, ArrowUp, ArrowUpDown, Check, ChevronDown, Filter, Search, X } from "lucide-react";
import { cx } from "./ui";

export type SortDir = "asc" | "desc";
type Val = string | number | boolean | null | undefined;

export interface SortDef<T> {
  label: string;
  get: (r: T) => Val;
  /** Dates and numbers sort descending first (newest, largest); text ascending. */
  type?: "text" | "number" | "date";
}

export interface FilterOption { value: string; label: ReactNode; icon?: ReactNode }
export type FilterDef<T> =
  /** Pick one or more values. Options are derived from the data when omitted. */
  | { kind: "enum"; key: string; label: string; get: (r: T) => string | string[] | null | undefined; options?: FilterOption[]; format?: (v: string) => ReactNode }
  /** Date presets (last 24 h, older than 90 days, never) and a custom range. */
  | { kind: "date"; key: string; label: string; get: (r: T) => string | null | undefined }
  /** A single on/off quick filter, such as "Needs attention". */
  | { kind: "toggle"; key: string; label: string; test: (r: T) => boolean };

export type DatePreset = "1d" | "7d" | "30d" | "90d" | "stale30" | "stale90" | "never" | "custom";
export interface DateRange { preset: DatePreset; from?: string; to?: string }
export type FilterValue = string[] | DateRange | boolean;
type FilterState = Record<string, FilterValue | undefined>;

const DATE_PRESETS: { id: DatePreset; label: string }[] = [
  { id: "1d", label: "Last 24 hours" },
  { id: "7d", label: "Last 7 days" },
  { id: "30d", label: "Last 30 days" },
  { id: "90d", label: "Last 90 days" },
  { id: "stale30", label: "More than 30 days ago" },
  { id: "stale90", label: "More than 90 days ago" },
  { id: "never", label: "Never" },
];

const DAY = 86_400_000;
function dateMatches(v: string | null | undefined, r: DateRange): boolean {
  const t = v ? new Date(v).getTime() : NaN;
  if (r.preset === "never") return !v || Number.isNaN(t);
  if (!v || Number.isNaN(t)) return false;
  const age = Date.now() - t;
  switch (r.preset) {
    case "1d": return age <= DAY;
    case "7d": return age <= 7 * DAY;
    case "30d": return age <= 30 * DAY;
    case "90d": return age <= 90 * DAY;
    case "stale30": return age > 30 * DAY;
    case "stale90": return age > 90 * DAY;
    case "custom": {
      if (r.from && t < new Date(`${r.from}T00:00:00`).getTime()) return false;
      if (r.to && t > new Date(`${r.to}T23:59:59.999`).getTime()) return false;
      return true;
    }
  }
}

function isActive(f: FilterDef<unknown>, v: FilterValue | undefined): boolean {
  if (v === undefined) return false;
  if (f.kind === "toggle") return v === true;
  if (f.kind === "enum") return Array.isArray(v) && v.length > 0;
  const r = v as DateRange;
  return r.preset !== "custom" || !!r.from || !!r.to;
}

/** Sort key for an IPv4 address, so 10.0.0.9 sorts before 10.0.0.10. */
export function ipKey(ip: string | null | undefined): string {
  if (!ip) return "";
  const p = ip.split(".");
  return p.length === 4 && p.every((x) => /^\d{1,3}$/.test(x)) ? p.map((x) => x.padStart(3, "0")).join(".") : ip;
}

function cmp(a: Val, b: Val, type: SortDef<unknown>["type"]): number {
  if (type === "date") {
    const x = a ? new Date(String(a)).getTime() : NaN, y = b ? new Date(String(b)).getTime() : NaN;
    return x - y;
  }
  if (typeof a === "number" && typeof b === "number") return a - b;
  if (typeof a === "boolean" && typeof b === "boolean") return Number(a) - Number(b);
  return String(a).localeCompare(String(b), undefined, { numeric: true, sensitivity: "base" });
}
const empty = (v: Val, type: SortDef<unknown>["type"]) =>
  v === null || v === undefined || v === "" || (type === "date" && Number.isNaN(new Date(String(v)).getTime()));

interface Stored { q: string; filters: FilterState; sort: { key: string; dir: SortDir } | null }
const store = (id: string) => `od.listview.${id}`;

export interface ListView<T> {
  rows: T[];
  total: number;
  q: string;
  setQ: (q: string) => void;
  filters: FilterDef<T>[];
  values: FilterState;
  setFilter: (key: string, v: FilterValue | undefined) => void;
  sorts: Record<string, SortDef<T>>;
  sort: { key: string; dir: SortDir } | null;
  toggleSort: (key: string) => void;
  setSort: (s: { key: string; dir: SortDir } | null) => void;
  activeCount: number;
  clear: () => void;
  data: T[];
  hasSearch: boolean;
}

/**
 * Search, filter and sort a list on the client. `id` keys the state in
 * sessionStorage, so filters survive opening an item and coming back.
 * `initial` (for example from a URL parameter) replaces any saved filters.
 */
export function useListView<T>(data: T[] | null | undefined, cfg: {
  id: string;
  search?: (r: T) => Val[];
  filters?: FilterDef<T>[];
  sorts?: Record<string, SortDef<T>>;
  defaultSort?: { key: string; dir: SortDir };
  initial?: FilterState;
}): ListView<T> {
  const [state, setState] = useState<Stored>(() => {
    let saved: Stored | null = null;
    try { const s = sessionStorage.getItem(store(cfg.id)); if (s) saved = JSON.parse(s) as Stored; } catch { /* ignore */ }
    const base: Stored = saved ?? { q: "", filters: {}, sort: cfg.defaultSort ?? null };
    if (cfg.initial) base.filters = { ...cfg.initial };
    if (base.sort && !cfg.sorts?.[base.sort.key]) base.sort = cfg.defaultSort ?? null;
    return base;
  });
  useEffect(() => { try { sessionStorage.setItem(store(cfg.id), JSON.stringify(state)); } catch { /* ignore */ } }, [cfg.id, state]);

  const all = data ?? [];
  const filters = cfg.filters ?? [];
  const sorts = cfg.sorts ?? {};
  const needle = state.q.trim().toLowerCase();

  let rows = all.filter((r) => {
    if (needle && cfg.search && !cfg.search(r).some((v) => v !== null && v !== undefined && String(v).toLowerCase().includes(needle))) return false;
    for (const f of filters) {
      const v = state.filters[f.key];
      if (!isActive(f as FilterDef<unknown>, v)) continue;
      if (f.kind === "toggle") { if (!f.test(r)) return false; }
      else if (f.kind === "enum") {
        const got = f.get(r);
        const vals = Array.isArray(got) ? got : got === null || got === undefined ? [] : [got];
        if (!vals.some((x) => (v as string[]).includes(x))) return false;
      } else if (!dateMatches(f.get(r), v as DateRange)) return false;
    }
    return true;
  });

  const s = state.sort && sorts[state.sort.key] ? state.sort : null;
  if (s) {
    const def = sorts[s.key]!;
    const sign = s.dir === "asc" ? 1 : -1;
    rows = rows.map((r, i) => ({ r, i, k: def.get(r) })).sort((a, b) => {
      const ea = empty(a.k, def.type), eb = empty(b.k, def.type);
      if (ea || eb) return ea === eb ? a.i - b.i : ea ? 1 : -1; // blanks last either way
      return cmp(a.k, b.k, def.type) * sign || a.i - b.i;
    }).map((x) => x.r);
  }

  const setFilter = (key: string, v: FilterValue | undefined) => setState((p) => ({ ...p, filters: { ...p.filters, [key]: v } }));
  const toggleSort = (key: string) => setState((p) => {
    const def = sorts[key];
    if (!def) return p;
    if (p.sort?.key === key) return { ...p, sort: { key, dir: p.sort.dir === "asc" ? "desc" : "asc" } };
    return { ...p, sort: { key, dir: def.type === "date" || def.type === "number" ? "desc" : "asc" } };
  });

  return {
    rows, total: all.length, data: all, q: state.q, hasSearch: !!cfg.search,
    setQ: (q) => setState((p) => ({ ...p, q })),
    filters, values: state.filters, setFilter,
    sorts, sort: s, toggleSort, setSort: (sort) => setState((p) => ({ ...p, sort })),
    activeCount: filters.filter((f) => isActive(f as FilterDef<unknown>, state.filters[f.key])).length + (needle ? 1 : 0),
    clear: () => setState((p) => ({ ...p, q: "", filters: {} })),
  };
}

// ------------------------------------------------------------------ UI

function usePopover() {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false); };
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", down);
    document.addEventListener("keydown", key);
    return () => { document.removeEventListener("mousedown", down); document.removeEventListener("keydown", key); };
  }, [open]);
  return { open, setOpen, ref };
}

const chip = (on: boolean) => cx(
  "inline-flex h-7 items-center gap-1 rounded-md border px-2.5 text-[13px] font-medium whitespace-nowrap transition",
  on ? "border-accent/40 bg-accent-soft text-accent" : "border-line bg-panel text-ink-2 hover:bg-panel-2 hover:text-ink",
);

function EnumFilter<T>({ f, view }: { f: Extract<FilterDef<T>, { kind: "enum" }>; view: ListView<T> }) {
  const p = usePopover();
  const sel = (view.values[f.key] as string[] | undefined) ?? [];
  const counts = new Map<string, number>();
  for (const r of view.data) {
    const g = f.get(r);
    for (const v of Array.isArray(g) ? g : g === null || g === undefined || g === "" ? [] : [g]) counts.set(v, (counts.get(v) ?? 0) + 1);
  }
  const options: FilterOption[] = f.options ?? [...counts.keys()].sort((a, b) => a.localeCompare(b, undefined, { numeric: true })).map((v) => ({ value: v, label: f.format ? f.format(v) : v }));
  const label = (v: string) => options.find((o) => o.value === v)?.label ?? v;
  const toggle = (v: string) => view.setFilter(f.key, sel.includes(v) ? sel.filter((x) => x !== v) : [...sel, v]);
  return (
    <div className="relative" ref={p.ref}>
      <button type="button" className={chip(sel.length > 0)} onClick={() => p.setOpen(!p.open)} aria-expanded={p.open}>
        {f.label}{sel.length === 1 ? <>: <span className="max-w-36 truncate">{label(sel[0]!)}</span></> : sel.length > 1 ? <span className="rounded-full bg-accent px-1.5 text-[11px] text-accent-ink">{sel.length}</span> : null}
        <ChevronDown className="size-3.5 opacity-70" />
      </button>
      {p.open && (
        <div className="anim-pop absolute left-0 top-8 z-30 w-64 rounded-lg border border-line bg-panel p-1 shadow-xl">
          <div className="max-h-72 overflow-y-auto">
            {options.length === 0 && <div className="px-2.5 py-2 text-xs text-ink-3">No values yet.</div>}
            {options.map((o) => {
              const on = sel.includes(o.value);
              return (
                <button key={o.value} type="button" onClick={() => toggle(o.value)}
                  className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm hover:bg-panel-2">
                  <span className={cx("grid size-4 shrink-0 place-items-center rounded border", on ? "border-accent bg-accent text-accent-ink" : "border-line")}>{on && <Check className="size-3" />}</span>
                  {o.icon}<span className="min-w-0 flex-1 truncate">{o.label}</span>
                  <span className="text-xs text-ink-3">{counts.get(o.value) ?? 0}</span>
                </button>
              );
            })}
          </div>
          {sel.length > 0 && <button type="button" onClick={() => view.setFilter(f.key, undefined)} className="mt-1 w-full border-t border-line px-2 pt-1.5 pb-1 text-left text-xs text-ink-3 hover:text-ink">Clear {f.label.toLowerCase()} filter</button>}
        </div>
      )}
    </div>
  );
}

function DateFilter<T>({ f, view }: { f: Extract<FilterDef<T>, { kind: "date" }>; view: ListView<T> }) {
  const p = usePopover();
  const v = view.values[f.key] as DateRange | undefined;
  const on = isActive(f as FilterDef<unknown>, v);
  const summary = !v || !on ? null : v.preset === "custom"
    ? [v.from, v.to].map((d) => d ? new Date(`${d}T00:00:00`).toLocaleDateString() : "…").join(" – ")
    : DATE_PRESETS.find((d) => d.id === v.preset)?.label;
  const set = (r: DateRange | undefined) => view.setFilter(f.key, r);
  return (
    <div className="relative" ref={p.ref}>
      <button type="button" className={chip(on)} onClick={() => p.setOpen(!p.open)} aria-expanded={p.open}>
        {f.label}{summary && <>: <span className="max-w-44 truncate">{summary}</span></>}<ChevronDown className="size-3.5 opacity-70" />
      </button>
      {p.open && (
        <div className="anim-pop absolute left-0 top-8 z-30 w-64 rounded-lg border border-line bg-panel p-1 shadow-xl">
          {DATE_PRESETS.map((d) => (
            <button key={d.id} type="button" onClick={() => { set(v?.preset === d.id ? undefined : { preset: d.id }); p.setOpen(false); }}
              className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm hover:bg-panel-2">
              <span className="grid size-4 place-items-center">{v?.preset === d.id && <Check className="size-3.5 text-accent" />}</span>{d.label}
            </button>
          ))}
          <div className="mt-1 space-y-1.5 border-t border-line px-2 pt-2 pb-1.5">
            <div className="text-xs font-medium text-ink-3">Custom range</div>
            <div className="flex items-center gap-1.5">
              <input type="date" aria-label="From" value={v?.preset === "custom" ? v.from ?? "" : ""} onChange={(e) => set({ preset: "custom", from: e.target.value || undefined, to: v?.preset === "custom" ? v.to : undefined })}
                className="h-7 min-w-0 flex-1 rounded-md border border-line bg-panel px-1.5 text-xs text-ink outline-none focus:border-accent" />
              <span className="text-xs text-ink-3">to</span>
              <input type="date" aria-label="To" value={v?.preset === "custom" ? v.to ?? "" : ""} onChange={(e) => set({ preset: "custom", to: e.target.value || undefined, from: v?.preset === "custom" ? v.from : undefined })}
                className="h-7 min-w-0 flex-1 rounded-md border border-line bg-panel px-1.5 text-xs text-ink outline-none focus:border-accent" />
            </div>
          </div>
          {on && <button type="button" onClick={() => { set(undefined); p.setOpen(false); }} className="mt-1 w-full border-t border-line px-2 pt-1.5 pb-1 text-left text-xs text-ink-3 hover:text-ink">Clear {f.label.toLowerCase()} filter</button>}
        </div>
      )}
    </div>
  );
}

/** Sort picker for lists that have no column headers. */
function SortPicker<T>({ view }: { view: ListView<T> }) {
  const keys = Object.keys(view.sorts);
  if (!keys.length) return null;
  const s = view.sort;
  return (
    <div className="flex items-center gap-1">
      <select value={s?.key ?? ""} aria-label="Sort by" onChange={(e) => {
        const k = e.target.value;
        if (!k) view.setSort(null); else { const t = view.sorts[k]!.type; view.setSort({ key: k, dir: t === "date" || t === "number" ? "desc" : "asc" }); }
      }} className="h-7 rounded-md border border-line bg-panel pl-2 pr-6 text-[13px] text-ink-2 outline-none focus:border-accent">
        {!s && <option value="">Sort by…</option>}
        {keys.map((k) => <option key={k} value={k}>Sort: {view.sorts[k]!.label}</option>)}
      </select>
      {s && (
        <button type="button" onClick={() => view.toggleSort(s.key)} className={chip(false)} title={s.dir === "asc" ? "Ascending" : "Descending"} aria-label={`Sort ${s.dir === "asc" ? "ascending" : "descending"}, click to reverse`}>
          {s.dir === "asc" ? <ArrowUp className="size-3.5" /> : <ArrowDown className="size-3.5" />}
        </button>
      )}
    </div>
  );
}

/**
 * Search box, filter chips, clear button and result count. Pass `sortPicker`
 * for lists without sortable column headers.
 */
export function ListToolbar<T>({ view, placeholder = "Search", extra, sortPicker, noun = ["item", "items"], className, compact, searchWidth }:
  { view: ListView<T>; placeholder?: string; extra?: ReactNode; sortPicker?: boolean; noun?: [string, string]; className?: string; compact?: boolean; searchWidth?: string }) {
  return (
    <div className={cx("flex flex-wrap items-center gap-2", compact ? "pb-2" : "border-b border-line p-3", className)}>
      {view.hasSearch && (
        <div className={cx("relative", searchWidth ?? (compact ? "w-56" : "w-72"))}>
          <Search className="pointer-events-none absolute left-2.5 top-2 size-4 text-ink-3" />
          <input value={view.q} onChange={(e) => view.setQ(e.target.value)} placeholder={placeholder} aria-label="Search"
            className="h-8 w-full rounded-md border border-line bg-panel pl-8 pr-7 text-sm text-ink placeholder:text-ink-3 outline-none transition focus:border-accent focus:ring-2 focus:ring-accent/20" />
          {view.q && <button type="button" onClick={() => view.setQ("")} aria-label="Clear search" className="absolute right-1.5 top-1.5 rounded p-0.5 text-ink-3 hover:text-ink"><X className="size-3.5" /></button>}
        </div>
      )}
      {view.filters.length > 0 && <Filter className="size-3.5 text-ink-3" aria-hidden />}
      {view.filters.map((f) => f.kind === "enum" ? <EnumFilter key={f.key} f={f} view={view} />
        : f.kind === "date" ? <DateFilter key={f.key} f={f} view={view} />
        : <button key={f.key} type="button" className={chip(view.values[f.key] === true)} onClick={() => view.setFilter(f.key, view.values[f.key] === true ? undefined : true)}>{f.label}</button>)}
      {view.activeCount > 0 && <button type="button" onClick={view.clear} className="inline-flex h-7 items-center gap-1 rounded-md px-2 text-[13px] text-ink-3 hover:bg-panel-2 hover:text-ink"><X className="size-3.5" />Clear all</button>}
      {extra}
      <div className="ml-auto flex items-center gap-3">
        {sortPicker && <SortPicker view={view} />}
        <span className="text-xs text-ink-3 whitespace-nowrap">
          {view.rows.length === view.total ? `${view.total} ${view.total === 1 ? noun[0] : noun[1]}` : `${view.rows.length} of ${view.total} ${noun[1]}`}
        </span>
      </div>
    </div>
  );
}

/** A table header cell. With `sort` set and a view passed to Table, clicking it sorts. */
export interface HeadCell { label: ReactNode; sort?: string; className?: string }
export const isHeadCell = (h: unknown): h is HeadCell =>
  typeof h === "object" && h !== null && "label" in h && !("$$typeof" in h);

export function SortHeader({ label, active, dir, onClick }: { label: ReactNode; active: boolean; dir?: SortDir; onClick: () => void }) {
  return (
    <button type="button" onClick={onClick} className={cx("group inline-flex items-center gap-1 uppercase tracking-wide transition hover:text-ink", active && "text-ink")}>
      {label}
      {active ? (dir === "asc" ? <ArrowUp className="size-3.5 text-accent" /> : <ArrowDown className="size-3.5 text-accent" />)
        : <ArrowUpDown className="size-3 opacity-0 transition group-hover:opacity-60" />}
    </button>
  );
}

/** "No results" body for a filtered list, with a button to clear filters. */
export function NoMatches({ view }: { view: { clear: () => void } }) {
  return (
    <div className="px-6 py-10 text-center">
      <p className="text-sm text-ink-2">Nothing matches these filters.</p>
      <button type="button" onClick={view.clear} className="mt-2 text-sm text-accent hover:underline">Clear filters</button>
    </div>
  );
}
