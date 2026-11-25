// Transport to the Management Server. Inside the Admin Center desktop app,
// requests go through the Rust side (which handles TLS pinning and the
// embedded server). In a plain browser during development, they use fetch.

import { invoke } from "@tauri-apps/api/core";

export const isDesktop = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
// In a browser the API is on the same origin (web mode, served by the
// Management Server). During development VITE_OD_SERVER points at the dev server.
const DEV_SERVER = (import.meta.env.VITE_OD_SERVER as string | undefined) ?? (import.meta.env.DEV ? "http://127.0.0.1:8080" : "");

export class ApiError extends Error {
  constructor(public status: number, message: string) { super(message); }
}

let token: string | null = localStorage.getItem("od.session");
let onUnauthorized: (() => void) | null = null;

export function setToken(t: string | null) {
  token = t;
  if (t) localStorage.setItem("od.session", t); else localStorage.removeItem("od.session");
}
export function getToken() { return token; }
export function setUnauthorizedHandler(fn: () => void) { onUnauthorized = fn; }

interface RawResponse { status: number; body: unknown }

async function send(method: string, path: string, body?: unknown): Promise<RawResponse> {
  if (isDesktop) {
    return invoke<RawResponse>("api_request", { method, path, body: body ?? null, token });
  }
  const res = await fetch(`${DEV_SERVER}/api/v1${path}`, {
    method,
    headers: { ...(body !== undefined ? { "content-type": "application/json" } : {}), ...(token ? { authorization: `Bearer ${token}` } : {}) },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  let parsed: unknown = null;
  try { parsed = text ? JSON.parse(text) : null; } catch { parsed = text; }
  return { status: res.status, body: parsed };
}

export async function api<T = unknown>(method: string, path: string, body?: unknown): Promise<T> {
  let res: RawResponse;
  try {
    res = await send(method, path, body);
  } catch (e) {
    throw new ApiError(0, typeof e === "string" ? e : (e as Error).message || "Can't reach the Management Server");
  }
  if (res.status >= 200 && res.status < 300) return res.body as T;
  const msg = (res.body && typeof res.body === "object" && "error" in res.body) ? String((res.body as { error: unknown }).error) : `Request failed (${res.status})`;
  if (res.status === 401 && token) onUnauthorized?.();
  throw new ApiError(res.status, msg);
}

export const get = <T,>(p: string) => api<T>("GET", p);
export const post = <T,>(p: string, b?: unknown) => api<T>("POST", p, b ?? {});
export const put = <T,>(p: string, b: unknown) => api<T>("PUT", p, b);
export const patch = <T,>(p: string, b: unknown) => api<T>("PATCH", p, b);
export const del = (p: string) => api<null>("DELETE", p);

// ---- Desktop-only connection management ----
export interface ConnectionState {
  mode: "local" | "remote" | null;
  url: string | null;
  fingerprint: string | null;
  data_dir: string | null;
  endpoint_url: string | null;
  error: string | null;
}
export const desktop = {
  status: () => invoke<ConnectionState>("connection_status"),
  hostLocally: (listenPort: number) => invoke<ConnectionState>("host_locally", { listenPort }),
  connectRemote: (url: string, fingerprint: string) => invoke<ConnectionState>("connect_remote", { url, fingerprint }),
  disconnect: () => invoke<ConnectionState>("disconnect"),
};

/** Saves binary data given as base64 (e.g. a backup file). Same locations as saveExport. */
export async function saveBinary(fileName: string, base64: string): Promise<string | null> {
  if (isDesktop) return invoke<string>("save_export", { fileName, contents: base64, base64: true });
  const bin = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
  const url = URL.createObjectURL(new Blob([bin], { type: "application/octet-stream" }));
  const a = Object.assign(document.createElement("a"), { href: url, download: fileName });
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  return null;
}

/** Saves a text export. Desktop: Downloads folder (returns the path). Browser: a normal download. */
export async function saveExport(fileName: string, contents: string): Promise<string | null> {
  if (isDesktop) return invoke<string>("save_export", { fileName, contents });
  const url = URL.createObjectURL(new Blob([contents], { type: "text/csv;charset=utf-8" }));
  const a = Object.assign(document.createElement("a"), { href: url, download: fileName });
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  return null;
}
