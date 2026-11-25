// Company branding: logo, accent color and product name from the Management
// Server, applied across the Admin Center (and the sign-in screen).
import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { get } from "./api";
import { useSession } from "./session";

export interface Branding { logo: string; accent: string; product_name: string; support_contact: string }
const EMPTY: Branding = { logo: "", accent: "", product_name: "", support_contact: "" };
const Ctx = createContext<{ brand: Branding; reload: () => void; preview: (b: Branding | null) => void }>({ brand: EMPTY, reload: () => {}, preview: () => {} });
export const useBranding = () => useContext(Ctx);

/** Readable text color on top of a hex color. */
function inkFor(hex: string) {
  const n = parseInt(hex.slice(1), 16);
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => { const c = v / 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; });
  return 0.2126 * r! + 0.7152 * g! + 0.0722 * b! > 0.4 ? "#0b1020" : "#ffffff";
}

function apply(b: Branding) {
  const root = document.documentElement.style;
  if (/^#[0-9a-f]{6}$/i.test(b.accent)) {
    root.setProperty("--accent", b.accent);
    root.setProperty("--accent-ink", inkFor(b.accent));
    root.setProperty("--accent-soft", `color-mix(in srgb, ${b.accent} 16%, var(--panel))`);
  } else {
    for (const v of ["--accent", "--accent-ink", "--accent-soft"]) root.removeProperty(v);
  }
  document.title = b.product_name ? `${b.product_name} · OpenDirectory` : "OpenDirectory Admin Center";
}

export function BrandingProvider({ children }: { children: ReactNode }) {
  const { connection, phase } = useSession();
  const [brand, setBrand] = useState<Branding>(EMPTY);
  const [previewing, setPreviewing] = useState<Branding | null>(null);
  const reload = useCallback(() => { get<Branding>("/branding").then(setBrand).catch(() => setBrand(EMPTY)); }, []);
  useEffect(() => { if (phase !== "loading" && phase !== "connect") reload(); }, [phase, connection?.url, reload]);
  const shown = previewing ?? brand;
  useEffect(() => apply(shown), [shown]);
  return <Ctx.Provider value={{ brand: shown, reload, preview: setPreviewing }}>{children}</Ctx.Provider>;
}

export function BrandMark({ className, sub }: { className?: string; sub?: ReactNode }) {
  const { brand } = useBranding();
  return (
    <>
      <img src={brand.logo || "/logo.svg"} className={`${className ?? "size-8"} object-contain`} alt="" />
      <div className="min-w-0 leading-tight">
        <div className="truncate text-[13px] font-semibold">{brand.product_name || "OpenDirectory"}</div>
        <div className="truncate text-xs text-ink-3">{sub ?? (brand.product_name ? "Powered by OpenDirectory" : "Admin Center")}</div>
      </div>
    </>
  );
}
