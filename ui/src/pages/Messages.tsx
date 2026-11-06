import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useInfiniteQuery, useQuery, useQueryClient } from "@tanstack/react-query";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowLeft, ArrowUp, Bell, BellOff, Download, File as FileIcon, Mail, MessageCircle, Paperclip, Phone, RotateCw, Upload } from "lucide-react";
import { api, blobUrl, errorText, native, type Attachment, type Chat } from "@/lib/bb";
import { addToThread, keys, type LocalMessage, type ThreadData } from "@/lib/events";
import { NARROW_BELOW, useApp, useWidth } from "@/lib/app";
import { bytes, dayLabel, sameDay, shortTime, time } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Avatar, Badge, Button, Card, Center, EmptyState, IconButton, SearchInput, Spinner, Textarea } from "@/components/ui";
import { Linkified } from "@/components/Linkified";
import { Lightbox } from "@/components/Lightbox";

const PAGE = 60;
const ROW = 64;

export const chatTitle = (chat: Chat) => (chat.display_name?.trim() ? chat.display_name : chat.participants.join(", ")) || "Unknown";

const isSms = (chat: Chat) => chat.service !== "IMessage";

export function MessagesPage() {
  const { connected, boot } = useApp();
  const [ref, width] = useWidth<HTMLDivElement>();
  const [selected, setSelected] = useState<string | null>(null);
  const chats = useQuery({ queryKey: keys.chats, queryFn: () => api.chats(300), enabled: connected, staleTime: Infinity });

  // Developer aid for screenshots.
  const opened = useRef(false);
  useEffect(() => {
    if (boot.dev_open_first_chat && !opened.current && chats.data?.length) {
      opened.current = true;
      setSelected((chats.data.find((c) => c.guid.startsWith("iMessage")) ?? chats.data[0]).guid);
    }
  }, [boot.dev_open_first_chat, chats.data]);

  const chat = chats.data?.find((c) => c.guid === selected) ?? null;
  const narrow = width > 0 && width < NARROW_BELOW;
  const listWidth = Math.min(360, Math.max(280, width * 0.3));

  const list = <ChatList chats={chats.data} loading={chats.isLoading} error={chats.error} selected={selected} onSelect={setSelected} />;
  return (
    <div ref={ref} className="flex h-full gap-4">
      {(!narrow || !chat) && (
        <Card className="flex min-w-0 flex-col overflow-hidden" style={narrow ? { flex: 1 } : { width: listWidth }}>
          {list}
        </Card>
      )}
      {(!narrow || chat) && (
        <Card className="flex min-w-0 flex-1 flex-col overflow-hidden">
          {chat ? (
            <Thread key={chat.guid} chat={chat} onBack={narrow ? () => setSelected(null) : undefined} />
          ) : (
            <EmptyState icon={MessageCircle} title={connected ? "Pick a conversation" : "Not connected"} hint={connected ? "Your iMessage and SMS chats from the Mac." : "Pair with your Mac in Settings."} />
          )}
        </Card>
      )}
    </div>
  );
}

function ChatList({ chats, loading, error, selected, onSelect }: { chats?: Chat[]; loading: boolean; error: unknown; selected: string | null; onSelect: (g: string) => void }) {
  const [filter, setFilter] = useState("");
  const scroller = useRef<HTMLDivElement>(null);
  const shown = useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (!chats || !q) return chats ?? [];
    return chats.filter((c) => chatTitle(c).toLowerCase().includes(q) || c.participants.some((p) => p.toLowerCase().includes(q)) || c.last_message?.text?.toLowerCase().includes(q));
  }, [chats, filter]);
  const virtual = useVirtualizer({ count: shown.length, getScrollElement: () => scroller.current, estimateSize: () => ROW, overscan: 8 });

  return (
    <>
      <div className="flex flex-col gap-3 p-4 pb-2">
        <div className="flex items-baseline justify-between">
          <h1 className="text-[15px] font-semibold text-fg">Messages</h1>
          {chats && <span className="text-xs text-subtle">{chats.length} chats</span>}
        </div>
        <SearchInput value={filter} onChange={setFilter} placeholder="Search" />
      </div>
      <div ref={scroller} className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {loading ? (
          <Center><Spinner /></Center>
        ) : error ? (
          <EmptyState icon={MessageCircle} title="Couldn't load chats" hint={errorText(error)} />
        ) : !shown.length ? (
          <EmptyState icon={MessageCircle} title={filter ? "No matches" : "No conversations"} />
        ) : (
          <div style={{ height: virtual.getTotalSize(), position: "relative" }}>
            {virtual.getVirtualItems().map((row) => {
              const chat = shown[row.index];
              return (
                <div key={chat.guid} style={{ position: "absolute", top: 0, left: 0, right: 0, height: row.size, transform: `translateY(${row.start}px)` }}>
                  <ChatRow chat={chat} active={chat.guid === selected} onSelect={onSelect} />
                </div>
              );
            })}
          </div>
        )}
      </div>
    </>
  );
}

const ChatRow = memo(function ChatRow({ chat, active, onSelect }: { chat: Chat; active: boolean; onSelect: (g: string) => void }) {
  const last = chat.last_message;
  const preview = last ? (last.text?.trim() || (last.attachments.length ? "Attachment" : "")) : "";
  return (
    <button
      onClick={() => onSelect(chat.guid)}
      className={cn("flex h-[60px] w-full items-center gap-3 rounded-control px-2.5 text-left transition-colors duration-150", active ? "bg-border" : "hover:bg-raised")}
    >
      <Avatar name={chatTitle(chat)} size={38} />
      <span className="min-w-0 flex-1">
        <span className="flex items-baseline gap-2">
          <span className="min-w-0 flex-1 truncate text-[13px] font-medium text-fg">{chatTitle(chat)}</span>
          {last && <span className="shrink-0 text-[11px] text-subtle">{shortTime(last.date)}</span>}
        </span>
        <span className="flex items-center gap-1.5">
          {isSms(chat) && <span className="shrink-0 rounded-full border border-border px-1.5 text-[10px] leading-4 text-subtle">SMS</span>}
          <span className="truncate text-xs text-muted">{last?.is_from_me ? `You: ${preview}` : preview}</span>
        </span>
      </span>
    </button>
  );
});

type Row =
  | { kind: "day"; key: string; label: string }
  | { kind: "msg"; key: string; m: LocalMessage; sender: string | null; tail: boolean };

let localId = 0;

function Thread({ chat, onBack }: { chat: Chat; onBack?: () => void }) {
  const { toast, config, setConfig } = useApp();
  const qc = useQueryClient();
  const scroller = useRef<HTMLDivElement>(null);
  const [draft, setDraft] = useState("");
  const [dragging, setDragging] = useState(false);
  const [viewing, setViewing] = useState<Attachment | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const atBottom = useRef(true);
  const group = chat.participants.length > 1;

  const thread = useInfiniteQuery({
    queryKey: keys.thread(chat.guid),
    queryFn: ({ pageParam }) => api.messages(chat.guid, PAGE, pageParam) as Promise<LocalMessage[]>,
    initialPageParam: null as number | null,
    getNextPageParam: (last) => (last.length < PAGE ? undefined : last[0]?.date),
    staleTime: Infinity,
  });

  const rows = useMemo<Row[]>(() => {
    const messages = thread.data ? thread.data.pages.slice().reverse().flat() : [];
    const out: Row[] = [];
    messages.forEach((m, i) => {
      const prev = messages[i - 1];
      const next = messages[i + 1];
      if (!prev || !sameDay(new Date(prev.date), new Date(m.date))) out.push({ kind: "day", key: `d${m.date}-${m.guid}`, label: dayLabel(m.date) });
      const sender = group && !m.is_from_me && (!prev || prev.sender !== m.sender || prev.is_from_me) ? m.sender : null;
      const tail = !next || next.is_from_me !== m.is_from_me || next.sender !== m.sender || next.date - m.date > 5 * 60_000;
      out.push({ kind: "msg", key: m.guid, m, sender, tail });
    });
    return out;
  }, [thread.data, group]);

  const virtual = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scroller.current,
    estimateSize: (i) => (rows[i].kind === "day" ? 40 : 44),
    getItemKey: (i) => rows[i].key,
    overscan: 10,
  });

  // Open at the newest message; follow new ones while the reader is at the bottom.
  const count = rows.length;
  const firstKey = rows[0]?.key;
  const prevFirst = useRef<string | undefined>(undefined);
  const prevCount = useRef(0);
  useLayoutEffect(() => {
    if (!count) return;
    const prepended = prevFirst.current !== undefined && firstKey !== prevFirst.current && count > prevCount.current;
    if (prepended) {
      const added = rows.findIndex((r) => r.key === prevFirst.current);
      if (added > 0) virtual.scrollToIndex(added, { align: "start" });
    } else if (prevCount.current === 0 || atBottom.current) {
      virtual.scrollToIndex(count - 1, { align: "end" });
      requestAnimationFrame(() => virtual.scrollToIndex(count - 1, { align: "end" }));
    }
    prevFirst.current = firstKey;
    prevCount.current = count;
  }, [count, firstKey]); // eslint-disable-line react-hooks/exhaustive-deps

  const onScroll = () => {
    const el = scroller.current;
    if (el) atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 120;
  };

  const send = async () => {
    const text = draft.trim();
    if (!text) return;
    setDraft("");
    atBottom.current = true;
    const local: LocalMessage = { guid: `local-${++localId}`, chat_guid: chat.guid, sender: null, is_from_me: true, text, date: Date.now(), attachments: [] };
    addToThread(qc, local);
    try {
      await api.sendText(chat.guid, text);
    } catch (err) {
      toast(`Not sent: ${errorText(err)}`);
      qc.setQueryData<ThreadData>(keys.thread(chat.guid), (d) => d && { ...d, pages: d.pages.map((p) => p.map((m) => (m.guid === local.guid ? { ...m, failed: true } : m))) });
    }
  };

  const sendFiles = (files: FileList | File[]) => {
    for (const file of Array.from(files)) {
      toast(`Sending ${file.name}…`);
      api.sendAttachment(chat.guid, file).catch((err) => toast(`${file.name} not sent: ${errorText(err)}`));
    }
  };

  const muted = config.notifications.muted_chats.includes(chat.guid);
  const toggleMute = () => {
    const n = config.notifications;
    const muted_chats = muted ? n.muted_chats.filter((g) => g !== chat.guid) : [...n.muted_chats, chat.guid];
    native.saveNotifications({ ...n, muted_chats }).then(setConfig, (e) => toast(errorText(e)));
  };

  const email = chat.my_address?.includes("@");
  return (
    <div
      className="relative flex h-full flex-col"
      onDragOver={(e) => {
        if (e.dataTransfer.types.includes("Files")) {
          e.preventDefault();
          setDragging(true);
        }
      }}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node)) setDragging(false);
      }}
      onDrop={(e) => {
        e.preventDefault();
        setDragging(false);
        if (e.dataTransfer.files.length) sendFiles(e.dataTransfer.files);
      }}
    >
      <div className="flex min-h-14 items-center gap-3 border-b border-border px-3 py-2">
        {onBack && <IconButton icon={ArrowLeft} label="Back" onClick={onBack} />}
        <Avatar name={chatTitle(chat)} size={34} />
        <div className="min-w-0 flex-1">
          <div className="truncate text-[14px] font-semibold text-fg">{chatTitle(chat)}</div>
          {chat.participants.join(", ") !== chatTitle(chat) && <div className="truncate text-xs text-subtle">{chat.participants.join(", ")}</div>}
        </div>
        <div className="hidden items-center gap-1.5 sm:flex">
          <Badge>{chat.service === "IMessage" ? "iMessage" : chat.service === "Sms" ? "SMS" : "RCS"}</Badge>
          {chat.my_address && (
            <Badge
              icon={email ? Mail : Phone}
              dot={email ? "var(--color-warning)" : undefined}
              title={email ? "This chat sends from your email. See Settings → Sending address to use your phone number." : "Replies go out from this address"}
            >
              {chat.my_address}
            </Badge>
          )}
        </div>
        <IconButton icon={muted ? BellOff : Bell} label={muted ? "Unmute notifications" : "Mute notifications"} onClick={toggleMute} className={muted ? "text-fg" : undefined} />
      </div>

      <div ref={scroller} onScroll={onScroll} className="min-h-0 flex-1 overflow-y-auto px-4">
        {thread.isLoading ? (
          <Center><Spinner /></Center>
        ) : thread.error ? (
          <EmptyState icon={MessageCircle} title="Couldn't load messages" hint={errorText(thread.error)} />
        ) : !rows.length ? (
          <EmptyState icon={MessageCircle} title="No messages yet" />
        ) : (
          <>
            <div className="flex h-12 items-center justify-center">
              {thread.hasNextPage ? (
                <Button variant="outline" size="sm" icon={RotateCw} busy={thread.isFetchingNextPage} onClick={() => void thread.fetchNextPage()}>
                  Load earlier
                </Button>
              ) : (
                <span className="text-[11px] text-subtle">Beginning of conversation</span>
              )}
            </div>
            <div style={{ height: virtual.getTotalSize(), position: "relative" }}>
              {virtual.getVirtualItems().map((item) => {
                const row = rows[item.index];
                return (
                  <div key={item.key} data-index={item.index} ref={virtual.measureElement} style={{ position: "absolute", top: 0, left: 0, right: 0, transform: `translateY(${item.start}px)` }}>
                    {row.kind === "day" ? (
                      <div className="py-3 text-center text-[11px] font-medium text-subtle">{row.label}</div>
                    ) : (
                      <Bubble m={row.m} sms={isSms(chat)} sender={row.sender} tail={row.tail} onView={setViewing} />
                    )}
                  </div>
                );
              })}
            </div>
            <div className="h-3" />
          </>
        )}
      </div>

      <div className="flex items-end gap-2 border-t border-border p-3">
        <input ref={fileInput} type="file" multiple hidden onChange={(e) => e.target.files && (sendFiles(e.target.files), (e.target.value = ""))} />
        <IconButton icon={Paperclip} label="Attach files" onClick={() => fileInput.current?.click()} />
        <AutoTextarea value={draft} onChange={setDraft} onSubmit={send} placeholder={chat.service === "IMessage" ? "iMessage" : "Text message"} />
        <IconButton icon={ArrowUp} label="Send" variant="primary" disabled={!draft.trim()} onClick={send} />
      </div>

      {dragging && (
        <div className="pointer-events-none absolute inset-2 z-10 flex items-center justify-center rounded-card border border-dashed border-border-strong bg-bg/80 animate-fade">
          <div className="flex flex-col items-center gap-2 text-fg">
            <Upload className="size-6" />
            <span className="text-sm font-medium">Drop to send to {chatTitle(chat)}</span>
          </div>
        </div>
      )}
      {viewing && <Lightbox kind="Attachment" id={viewing.guid} name={viewing.file_name ?? "image.jpg"} onClose={() => setViewing(null)} />}
    </div>
  );
}

function AutoTextarea({ value, onChange, onSubmit, placeholder }: { value: string; onChange: (v: string) => void; onSubmit: () => void; placeholder: string }) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${Math.min(160, el.scrollHeight)}px`;
  }, [value]);
  return (
    <Textarea
      ref={ref}
      rows={1}
      autoFocus
      value={value}
      placeholder={placeholder}
      onChange={(e) => onChange(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
          e.preventDefault();
          onSubmit();
        }
      }}
      className="max-h-40 min-h-9 flex-1 rounded-[18px] py-[7px] leading-5"
    />
  );
}

const Bubble = memo(function Bubble({ m, sms, sender, tail, onView }: { m: LocalMessage; sms: boolean; sender: string | null; tail: boolean; onView: (a: Attachment) => void }) {
  const mine = m.is_from_me;
  const style = mine ? (sms ? "border border-border-strong text-fg" : "bg-primary text-primary-fg") : "bg-raised text-fg";
  const text = m.text?.replace(/￼/g, "").trim();
  return (
    <div className={cn("flex flex-col", mine ? "items-end" : "items-start", tail ? "pb-2" : "pb-0.5")}>
      {sender && <span className="mb-0.5 px-3 text-[11px] text-subtle">{sender}</span>}
      {m.attachments.map((a) => (
        <AttachmentView key={a.guid} a={a} mine={mine} onView={onView} />
      ))}
      {text && (
        <div title={time(m.date)} className={cn("selectable max-w-[min(78%,560px)] rounded-[18px] px-3.5 py-[7px] text-[13.5px] leading-[1.4] whitespace-pre-wrap break-words", style, m.failed && "opacity-60")}>
          <Linkified text={text} />
        </div>
      )}
      {tail && (
        <span className={cn("mt-0.5 px-2 text-[10.5px]", m.failed ? "text-danger" : "text-subtle")}>
          {m.failed ? "Not delivered" : m.guid.startsWith("local-") ? "Sending…" : time(m.date)}
        </span>
      )}
    </div>
  );
});

function AttachmentView({ a, mine, onView }: { a: Attachment; mine: boolean; onView: (a: Attachment) => void }) {
  const { toast } = useApp();
  const [failed, setFailed] = useState(false);
  const image = a.mime_type?.startsWith("image/") && !failed;
  const save = useCallback(() => {
    native.saveBlob("Attachment", a.guid, a.file_name ?? "attachment").then((p) => toast(`Saved to ${p}`), (e) => toast(errorText(e)));
  }, [a, toast]);
  if (image) {
    return (
      <button onClick={() => onView(a)} className="mb-0.5 overflow-hidden rounded-[18px] border border-border bg-raised">
        <img src={blobUrl("Attachment", a.guid)} alt={a.file_name ?? ""} loading="lazy" decoding="async" onError={() => setFailed(true)} className="block max-h-72 max-w-[min(280px,60vw)] min-h-16 min-w-16 object-cover" />
      </button>
    );
  }
  return (
    <button onClick={save} className={cn("mb-0.5 flex max-w-72 items-center gap-3 rounded-[18px] border px-3 py-2 text-left", mine ? "border-border-strong" : "border-border bg-raised")}>
      <FileIcon className="size-5 shrink-0 text-muted" />
      <span className="min-w-0">
        <span className="block truncate text-[13px] text-fg">{a.file_name ?? "Attachment"}</span>
        <span className="block text-[11px] text-subtle">{bytes(a.total_bytes)}</span>
      </span>
      <Download className="size-4 shrink-0 text-subtle" />
    </button>
  );
}
