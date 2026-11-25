import { useCallback, useEffect, useRef, useState } from "react";
import { get } from "./api";

/** Fetch a GET endpoint, with reload and optional polling. */
export function useQuery<T>(path: string | null, opts: { poll?: number } = {}) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const seq = useRef(0);

  const load = useCallback(async (quiet = false) => {
    if (!path) return;
    const n = ++seq.current;
    if (!quiet) setLoading(true);
    try {
      const d = await get<T>(path);
      if (n === seq.current) { setData(d); setError(null); }
    } catch (e) {
      if (n === seq.current) setError((e as Error).message);
    } finally {
      if (n === seq.current) setLoading(false);
    }
  }, [path]);

  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    if (!opts.poll || !path) return;
    const t = setInterval(() => void load(true), opts.poll);
    return () => clearInterval(t);
  }, [opts.poll, path, load]);

  return { data, error, loading, reload: () => load(true), setData };
}

export function useLocalStorage<T>(key: string, initial: T) {
  const [v, setV] = useState<T>(() => {
    try { const s = localStorage.getItem(key); return s ? (JSON.parse(s) as T) : initial; } catch { return initial; }
  });
  useEffect(() => { localStorage.setItem(key, JSON.stringify(v)); }, [key, v]);
  return [v, setV] as const;
}
