import { useEffect, useRef, useState } from "react";
import { Send } from "lucide-react";
import { useQuery } from "../lib/hooks";
import { post } from "../lib/api";
import type { ChatMessage } from "../lib/types";
import { ago } from "../lib/format";
import { Button, Loading, Textarea, cx, useToast } from "./ui";

/** One help desk conversation with the person at an endpoint. */
export function HelpDeskChat({ endpointId, hostname }: { endpointId: string; hostname: string }) {
  const toast = useToast();
  const msgs = useQuery<ChatMessage[]>(`/endpoints/${endpointId}/chat`, { poll: 4000 });
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const end = useRef<HTMLDivElement>(null);
  const count = msgs.data?.length ?? 0;
  useEffect(() => { end.current?.scrollIntoView({ block: "end" }); }, [count]);
  const send = async () => {
    const t = text.trim();
    if (!t) return;
    setBusy(true);
    try { await post(`/endpoints/${endpointId}/chat`, { text: t }); setText(""); void msgs.reload(); }
    catch (e) { toast("bad", (e as Error).message); } finally { setBusy(false); }
  };
  return (
    <div className="flex h-[480px] flex-col">
      <div className="flex-1 space-y-2.5 overflow-auto p-4">
        {msgs.loading && !msgs.data ? <Loading /> : count === 0 ? (
          <p className="pt-10 text-center text-sm text-ink-3">No messages with {hostname} yet. Replies appear on the device within seconds.</p>
        ) : msgs.data!.map((m) => (
          <div key={m.id} className={cx("max-w-[75%] rounded-2xl border px-3.5 py-2 text-sm", m.from_it ? "ml-auto border-transparent bg-accent text-white" : "border-line bg-panel-2")}>
            <div className="whitespace-pre-wrap break-words">{m.text}</div>
            <div className={cx("mt-1 text-[11px]", m.from_it ? "text-white/75" : "text-ink-3")}>{m.from_it ? `${m.author} (IT)` : m.author} · {ago(m.at)}</div>
          </div>
        ))}
        <div ref={end} />
      </div>
      <div className="flex gap-2 border-t border-line p-3">
        <Textarea rows={2} value={text} maxLength={4000} placeholder="Reply to the user…" onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void send(); } }} />
        <Button variant="primary" icon={<Send className="size-4" />} loading={busy} disabled={!text.trim()} onClick={send}>Send</Button>
      </div>
    </div>
  );
}
