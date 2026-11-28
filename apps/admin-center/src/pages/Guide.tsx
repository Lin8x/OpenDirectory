import { useEffect, useMemo, useRef, useState } from "react";
import { Link, NavLink, useParams, useSearchParams } from "react-router-dom";
import { BookOpen, Search } from "lucide-react";
import { CHAPTERS, chapter, guidePath, headingId } from "../lib/guide";
import { Markdown } from "../components/Markdown";
import { Input, cx } from "../components/ui";

const PARTS: [string, string][] = [["ideas", "Part 1: The ideas"], ["doing", "Part 2: Doing it"], ["reference", "Reference"]];

/** The Beginner's Guide, bundled with the app (works offline). */
export default function Guide() {
  const { slug = "" } = useParams();
  const [params] = useSearchParams();
  const section = params.get("s");
  const ch = chapter(slug) ?? chapter("")!;
  const top = useRef<HTMLDivElement>(null);
  const [q, setQ] = useState("");

  useEffect(() => {
    const el = section ? document.getElementById(section) : null;
    (el ?? top.current)?.scrollIntoView({ block: "start" });
  }, [ch.slug, section]);

  const hits = useMemo(() => {
    const n = q.trim().toLowerCase();
    if (n.length < 2) return [];
    return CHAPTERS.flatMap((c) => c.body.split("\n").filter((l) => /^#{2,4} /.test(l) && l.toLowerCase().includes(n))
      .map((l) => ({ slug: c.slug, chapter: c.title, heading: l.replace(/^#+\s*/, "") })))
      .concat(CHAPTERS.flatMap((c) => c.body.split("\n").filter((l) => l.startsWith("| **") && l.toLowerCase().includes(n) && c.slug === "glossary")
        .map((l) => ({ slug: c.slug, chapter: "Glossary", heading: l.match(/\*\*(.+?)\*\*/)?.[1] ?? "" }))))
      .slice(0, 12);
  }, [q]);

  return (
    <div ref={top} className="grid gap-6 lg:grid-cols-[240px_minmax(0,1fr)]">
      <aside className="lg:sticky lg:top-0 lg:self-start">
        <Link to={guidePath("")} className="mb-3 flex items-center gap-2 text-[15px] font-semibold"><BookOpen className="size-4 text-accent" />Beginner's Guide</Link>
        <div className="relative mb-3">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-ink-3" />
          <Input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search the guide" className="pl-8" />
        </div>
        {q.trim().length >= 2 ? (
          <ul className="space-y-1 text-sm">
            {hits.length === 0 && <li className="px-2 text-ink-3">No matches</li>}
            {hits.map((h, i) => (
              <li key={i}><Link to={h.slug === "glossary" ? guidePath("glossary") : guidePath(h.slug, headingId(h.heading))} onClick={() => setQ("")}
                className="block rounded-md px-2 py-1.5 hover:bg-panel-2"><span className="block font-medium">{h.heading}</span><span className="text-xs text-ink-3">{h.chapter}</span></Link></li>
            ))}
          </ul>
        ) : (
          <nav className="space-y-4 text-[13.5px]">
            {PARTS.map(([part, label]) => (
              <div key={part}>
                <div className="mb-1 px-2 text-[11px] font-semibold text-ink-3">{label}</div>
                {CHAPTERS.filter((c) => c.part === part).map((c) => (
                  <NavLink key={c.slug} to={guidePath(c.slug)} end className={({ isActive }) => cx("block rounded-md px-2 py-1.5 leading-snug", isActive ? "bg-accent-soft font-medium text-accent" : "text-ink-2 hover:bg-panel-2")}>{c.title}</NavLink>
                ))}
              </div>
            ))}
          </nav>
        )}
      </aside>
      <article className="min-w-0 max-w-[820px] rounded-xl border border-line bg-panel px-6 py-6 shadow-sm md:px-9">
        <Markdown source={ch.body} />
      </article>
    </div>
  );
}
