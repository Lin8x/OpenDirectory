import { useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { LifeBuoy } from "lucide-react";
import { useQuery } from "../lib/hooks";
import type { ChatThread } from "../lib/types";
import { ago } from "../lib/format";
import { Badge, Card, Loading, cx } from "../components/ui";
import { HelpDeskChat } from "../components/HelpDeskChat";

/** Help desk: every conversation users started from their devices. */
export default function HelpDesk() {
  const threads = useQuery<ChatThread[]>("/helpdesk", { poll: 5000 });
  const [params, setParams] = useSearchParams();
  const [sel, setSel] = useState<string | null>(params.get("endpoint"));
  const list = threads.data ?? [];
  const cur = list.find((t) => t.endpoint_id === sel) ?? null;
  const pick = (id: string) => { setSel(id); setParams({ endpoint: id }, { replace: true }); };
  return (
    <div className="space-y-5">
      <div>
        <h1 className="text-[22px] font-semibold tracking-tight">Help Desk</h1>
        <p className="text-sm text-ink-2">Chats people started from their devices. Turn it on with the <Link to="/gpos" className="text-accent hover:underline">Help desk chat</Link> GPO setting; users then get a "Contact IT" shortcut (or the Help screen in the Android app).</p>
      </div>
      <div className="grid gap-5 lg:grid-cols-[320px_minmax(0,1fr)]">
        <Card title="Conversations" padded={false}>
          {threads.loading && !threads.data ? <Loading /> : list.length === 0 ? (
            <div className="p-6 text-center text-sm text-ink-3"><LifeBuoy className="mx-auto mb-2 size-6" />No conversations yet.</div>
          ) : (
            <ul className="divide-y divide-line">
              {list.map((t) => (
                <li key={t.endpoint_id}>
                  <button onClick={() => pick(t.endpoint_id)} className={cx("w-full px-4 py-3 text-left hover:bg-panel-2", sel === t.endpoint_id && "bg-panel-2")}>
                    <div className="flex items-center justify-between gap-2">
                      <span className="truncate text-sm font-medium">{t.hostname || "Retired endpoint"}</span>
                      {t.unread > 0 && <Badge tone="accent">{t.unread} new</Badge>}
                    </div>
                    <div className="mt-0.5 truncate text-xs text-ink-3">{t.last.from_it ? "You: " : `${t.last.author}: `}{t.last.text}</div>
                    <div className="text-[11px] text-ink-3">{ago(t.last.at)}</div>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </Card>
        <Card title={cur ? <Link to={`/endpoints/${cur.endpoint_id}`} className="hover:underline">{cur.hostname}</Link> : "Select a conversation"} padded={false}>
          {sel ? <HelpDeskChat endpointId={sel} hostname={cur?.hostname ?? "this endpoint"} /> : <p className="p-10 text-center text-sm text-ink-3">Pick a conversation on the left.</p>}
        </Card>
      </div>
    </div>
  );
}
