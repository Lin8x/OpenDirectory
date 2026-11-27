// A small Markdown renderer for the bundled guide. It covers the subset the
// guide uses (headings, paragraphs, nested lists, tables, quotes, code,
// images, links, <details>) and builds React elements, never raw HTML.
import { Fragment, type ReactNode } from "react";
import { Link } from "react-router-dom";
import { ExternalLink } from "lucide-react";
import { guideImage, guidePath, headingId } from "../lib/guide";

const REPO_BASE = "https://github.com/Lin8x/OpenDirectory/blob/main/apps/admin-center/guide/";

// ------------------------------------------------------------------ inline
const INLINE = /(`[^`]+`)|(!\[[^\]]*\]\([^)\s]+\))|(\[(?:[^\]]|\][^(])*?\]\([^)\s]+\))|(\*\*(?:[^*]|\*(?!\*))+?\*\*)|(\*(?!\*)(?:\*\*[^*]+\*\*|[^*])+?\*(?!\*))|(<\/?b>)|(<br\s*\/?>)/;

function inline(text: string, key = "i"): ReactNode[] {
  const out: ReactNode[] = [];
  let rest = text;
  let n = 0;
  while (rest) {
    const m = INLINE.exec(rest);
    if (!m) { out.push(rest); break; }
    if (m.index) out.push(rest.slice(0, m.index));
    const tok = m[0];
    const k = `${key}-${n++}`;
    if (m[1]) out.push(<code key={k} className="rounded bg-panel-2 px-1 py-0.5 font-mono text-[0.85em]">{tok.slice(1, -1)}</code>);
    else if (m[2]) {
      const [, alt, src] = tok.match(/^!\[([^\]]*)\]\(([^)]+)\)$/)!;
      out.push(<img key={k} src={guideImage(src) ?? src} alt={alt} loading="lazy" className="my-2 block w-full rounded-lg border border-line shadow-sm" />);
    } else if (m[3]) {
      const mm = tok.match(/^\[([\s\S]*)\]\(([^)]+)\)$/)!;
      out.push(<GuideLink key={k} href={mm[2]}>{inline(mm[1], k)}</GuideLink>);
    } else if (m[4]) out.push(<strong key={k} className="font-semibold text-ink">{inline(tok.slice(2, -2), k)}</strong>);
    else if (m[5]) out.push(<em key={k}>{inline(tok.slice(1, -1), k)}</em>);
    else if (m[7]) out.push(<br key={k} />);
    // m[6] (<b> tags) is dropped: the surrounding text is already bold where it matters.
    rest = rest.slice(m.index + tok.length);
  }
  return out;
}

function GuideLink({ href, children }: { href: string; children: ReactNode }) {
  const cls = "text-accent hover:underline";
  const local = href.match(/^([\w-]+)\.md(?:#(.+))?$/);
  if (local) return <Link to={guidePath(local[1] === "README" ? "" : local[1], local[2])} className={cls}>{children}</Link>;
  if (href.startsWith("#")) return <Link to={`?s=${encodeURIComponent(href.slice(1))}`} className={cls}>{children}</Link>;
  const url = /^https?:/.test(href) ? href : new URL(href, REPO_BASE).toString();
  return <a href={url} target="_blank" rel="noreferrer" className={`${cls} inline-flex items-center gap-0.5`}>{children}<ExternalLink className="size-3" /></a>;
}

// ------------------------------------------------------------------ blocks
const LIST = /^(\s*)([-*]|\d+\.)\s+/;
const startsBlock = (l: string) => /^(#{1,6}\s|```|>|\||---\s*$|<details>)/.test(l) || LIST.test(l);
const dedent = (lines: string[], n: number) => lines.map((l) => l.slice(Math.min(n, l.length - l.trimStart().length)));

export function Markdown({ source }: { source: string }) {
  return <div className="text-[15px] leading-relaxed text-ink-2">{blocks(source.replace(/\r/g, "").split("\n"), "b")}</div>;
}

function blocks(lines: string[], key: string): ReactNode[] {
  const out: ReactNode[] = [];
  let i = 0;
  const k = () => `${key}-${out.length}`;
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) { i++; continue; }

    if (line.startsWith("```")) {
      const body: string[] = [];
      for (i++; i < lines.length && !lines[i].startsWith("```"); i++) body.push(lines[i]);
      i++;
      out.push(<pre key={k()} className="my-4 overflow-x-auto rounded-lg border border-line bg-panel-2 p-3 font-mono text-[12.5px] leading-snug text-ink">{body.join("\n")}</pre>);
      continue;
    }

    const h = line.match(/^(#{1,6})\s+(.*)$/);
    if (h) {
      const level = h[1].length;
      const id = headingId(h[2]);
      const cls = ["", "mb-2 text-[26px] font-semibold tracking-tight text-ink", "mb-2 mt-9 text-xl font-semibold text-ink", "mb-1.5 mt-7 text-[17px] font-semibold text-ink", "mb-1 mt-5 font-semibold text-ink"][Math.min(level, 4)];
      const Tag = `h${Math.min(level + 0, 4)}` as "h1";
      out.push(<Tag key={k()} id={id} className={`${cls} scroll-mt-4`}>{inline(h[2], k())}</Tag>);
      i++;
      continue;
    }

    if (/^---\s*$/.test(line)) { out.push(<hr key={k()} className="my-7 border-line" />); i++; continue; }

    if (line.startsWith(">")) {
      const body: string[] = [];
      for (; i < lines.length && lines[i].startsWith(">"); i++) body.push(lines[i].replace(/^> ?/, ""));
      out.push(<div key={k()} className="my-5 rounded-lg border border-accent/25 bg-accent-soft/50 px-4 py-1 text-[14px]">{blocks(body, k())}</div>);
      continue;
    }

    if (line.startsWith("|")) {
      const rows: string[][] = [];
      for (; i < lines.length && lines[i].startsWith("|"); i++) {
        if (/^\|[\s:|-]+\|\s*$/.test(lines[i])) continue;
        rows.push(lines[i].trim().replace(/^\||\|$/g, "").split(/(?<!\\)\|/).map((c) => c.trim()));
      }
      const [head, ...body] = rows;
      out.push(
        <div key={k()} className="my-4 overflow-x-auto rounded-lg border border-line">
          <table className="w-full text-left text-[13.5px]">
            <thead className="bg-panel-2 text-ink"><tr>{head.map((c, j) => <th key={j} className="px-3 py-2 font-semibold">{inline(c, `${k()}h${j}`)}</th>)}</tr></thead>
            <tbody className="divide-y divide-line">{body.map((r, ri) => <tr key={ri}>{r.map((c, j) => <td key={j} className="px-3 py-2 align-top">{inline(c, `${k()}r${ri}c${j}`)}</td>)}</tr>)}</tbody>
          </table>
        </div>,
      );
      continue;
    }

    if (line.startsWith("<details>")) {
      const body: string[] = [];
      let summary = "More";
      for (i++; i < lines.length && !lines[i].startsWith("</details>"); i++) {
        const s = lines[i].match(/<summary>(.*)<\/summary>/);
        if (s) summary = s[1].replace(/<[^>]+>/g, "");
        else body.push(lines[i]);
      }
      i++;
      out.push(
        <details key={k()} className="my-5 rounded-lg border border-line px-4 py-2">
          <summary className="cursor-pointer font-semibold text-ink">{summary}</summary>
          <div className="pb-2">{blocks(body, k())}</div>
        </details>,
      );
      continue;
    }

    const lm = line.match(LIST);
    if (lm) {
      const indent = lm[1].length;
      const ordered = /\d/.test(lm[2]);
      const items: string[][] = [];
      while (i < lines.length) {
        const cur = lines[i];
        const m = cur.match(LIST);
        if (m && m[1].length === indent && /\d/.test(m[2]) === ordered) {
          items.push([cur.slice(m[0].length)]);
          i++;
          continue;
        }
        const ind = cur.length - cur.trimStart().length;
        if (items.length && (cur.trim() === "" || ind > indent)) {
          // A blank line ends the list unless the next line is still indented.
          if (cur.trim() === "") {
            const next = lines.slice(i + 1).find((l) => l.trim());
            const nextInd = next ? next.length - next.trimStart().length : 0;
            const nextItem = next?.match(LIST);
            if (!next || (nextInd <= indent && !(nextItem && nextItem[1].length === indent))) break;
          }
          items[items.length - 1].push(cur);
          i++;
          continue;
        }
        break;
      }
      const start = ordered ? Number(lm[2].slice(0, -1)) : undefined;
      const List = ordered ? "ol" : "ul";
      out.push(
        <List key={k()} start={start} className={`my-3 space-y-1.5 pl-6 ${ordered ? "list-decimal" : "list-disc"} marker:text-ink-3`}>
          {items.map((it, j) => {
            const [first, ...more] = it;
            const nested = dedent(more, indent + 3);
            const simple = nested.every((l) => !l.trim());
            return <li key={j} className="pl-1">{simple ? inline(first, `${k()}l${j}`) : <>{blocks([first, ...nested], `${k()}l${j}`).map((b, x) => <Fragment key={x}>{b}</Fragment>)}</>}</li>;
          })}
        </List>,
      );
      continue;
    }

    const para: string[] = [];
    for (; i < lines.length && lines[i].trim() && !(para.length && startsBlock(lines[i])); i++) para.push(lines[i].trim());
    const text = para.join(" ");
    const caption = /^\*(?!\*).*\*$/.test(text);
    out.push(<p key={k()} className={caption ? "-mt-1 mb-5 text-center text-[13px] text-ink-3" : "my-3"}>{inline(text, k())}</p>);
  }
  return out;
}
