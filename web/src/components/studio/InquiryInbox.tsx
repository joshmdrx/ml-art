"use client";

/**
 * Two-pane studio inquiry inbox.
 *
 * Layout mirrors Gmail / Superhuman / Etsy's seller inbox:
 *   - Left pane (fixed 20rem on md+): compact list of inquiries with
 *     unread indicators, sorted by latest activity.
 *   - Right pane: the selected thread (full conversation + reply form)
 *     or an empty-state prompt when nothing is selected.
 *
 * Selection is URL-driven via `?id=<inquiry_id>` so bookmarks, browser
 * back / forward, and refresh all Just Work. Status filter (`?status=`)
 * is preserved across selections.
 *
 * Mobile (<md): the two panes stack. With no `?id=`, only the list is
 * visible. With `?id=` set, only the thread is visible + a "← All
 * inquiries" back link at the top.
 *
 * Read/unread behavior changed here (previously T-011 Phase 4b marked
 * ALL loaded inquiries as read on page render). Now: only the
 * currently-selected inquiry is marked as read when opened. Preserves
 * the "which of these still needs my attention" signal.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import clsx from "clsx";

import type { StudioInquiry, StudioInquiryReply } from "@/lib/api";
import { toUserMessage } from "@/lib/reportError";

interface Props {
  initialItems: StudioInquiry[];
  selectedId: string | null;
}

export function InquiryInbox({ initialItems, selectedId }: Props) {
  const searchParams = useSearchParams();
  const statusParam = searchParams.get("status");

  // Local copy so optimistic reply append doesn't fight the server
  // payload. Re-syncs when the server pushes a fresh list (filter
  // change flips the initialItems identity).
  const [prevServerItems, setPrevServerItems] = useState(initialItems);
  const [items, setItems] = useState(initialItems);
  if (prevServerItems !== initialItems) {
    setPrevServerItems(initialItems);
    setItems(initialItems);
  }

  // Sort by latest activity (newest reply, or created_at if no
  // replies). Recomputed only when items array identity changes, so
  // an optimistic reply append doesn't scramble the list mid-session.
  const sortedItems = useMemo(() => {
    return [...items].sort((a, b) => latestActivity(b) - latestActivity(a));
  }, [items]);

  const selected: StudioInquiry | null = selectedId
    ? items.find((i) => i.id === selectedId) ?? null
    : null;

  // Auto-mark-as-read the currently-selected inquiry when it's opened
  // + still unread. Fires once per (id, initialItems) pair so
  // navigating away and back doesn't refire.
  const markedRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    if (!selected || selected.read_at !== null) return;
    if (markedRef.current.has(selected.id)) return;
    markedRef.current.add(selected.id);
    fetch("/api/studio/inquiries/read", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ ids: [selected.id] }),
    }).catch(() => {
      /* best-effort — silent */
    });
    // Optimistic local update so the list stops showing bold + dot.
    setItems((prev) =>
      prev.map((i) =>
        i.id === selected.id
          ? { ...i, read_at: new Date().toISOString() }
          : i,
      ),
    );
  }, [selected]);

  function onReplied(inquiryId: string, reply: StudioInquiryReply) {
    setItems((prev) =>
      prev.map((i) =>
        i.id === inquiryId
          ? { ...i, replies: [...i.replies, reply] }
          : i,
      ),
    );
  }

  const listHrefBase =
    statusParam && statusParam !== "all"
      ? `/studio/inquiries?status=${encodeURIComponent(statusParam)}`
      : "/studio/inquiries";

  return (
    <div className="md:grid md:grid-cols-[20rem_1fr] md:gap-6 md:min-h-[32rem]">
      {/* List pane */}
      <aside
        className={clsx(
          "md:block",
          selected ? "hidden md:block" : "block",
          "md:border-r md:border-border md:pr-4",
        )}
        aria-label="Inquiry list"
      >
        <InquiryList
          items={sortedItems}
          selectedId={selectedId}
          statusParam={statusParam}
        />
      </aside>

      {/* Thread pane */}
      <section
        className={clsx(
          "md:block",
          selected ? "block" : "hidden md:block",
        )}
        aria-label="Inquiry thread"
      >
        {selected ? (
          <>
            {/* Mobile-only back link — desktop uses list selection */}
            <div className="md:hidden mb-4">
              <Link
                href={listHrefBase}
                className="text-sm underline underline-offset-2 text-muted hover:text-foreground"
              >
                ← All inquiries
              </Link>
            </div>
            <InquiryThread inquiry={selected} onReplied={onReplied} />
          </>
        ) : (
          <div className="hidden md:flex items-center justify-center text-sm text-muted min-h-[24rem]">
            Select an inquiry to view the thread.
          </div>
        )}
      </section>
    </div>
  );
}

function InquiryList({
  items,
  selectedId,
  statusParam,
}: {
  items: StudioInquiry[];
  selectedId: string | null;
  statusParam: string | null;
}) {
  if (items.length === 0) {
    return (
      <p className="text-sm text-muted py-6 text-center">
        No inquiries match this filter.
      </p>
    );
  }
  return (
    <ul className="flex flex-col">
      {items.map((inq) => {
        const isSelected = inq.id === selectedId;
        const isUnread = inq.read_at === null;
        const href = buildRowHref(inq.id, statusParam);
        return (
          <li key={inq.id}>
            <InquiryListRow
              inquiry={inq}
              href={href}
              isSelected={isSelected}
              isUnread={isUnread}
            />
          </li>
        );
      })}
    </ul>
  );
}

function buildRowHref(id: string, statusParam: string | null): string {
  const params = new URLSearchParams();
  params.set("id", id);
  if (statusParam && statusParam !== "all") params.set("status", statusParam);
  return `/studio/inquiries?${params.toString()}`;
}

function InquiryListRow({
  inquiry,
  href,
  isSelected,
  isUnread,
}: {
  inquiry: StudioInquiry;
  href: string;
  isSelected: boolean;
  isUnread: boolean;
}) {
  const latest = new Date(latestActivity(inquiry));
  const latestReply = inquiry.replies[inquiry.replies.length - 1];
  const snippet = latestReply?.message ?? inquiry.message;

  return (
    <Link
      href={href}
      scroll={false}
      aria-current={isSelected ? "true" : undefined}
      className={clsx(
        "block border-l-2 pl-3 pr-2 py-3 transition-colors",
        isSelected
          ? "border-foreground bg-background"
          : "border-transparent hover:bg-background",
      )}
    >
      <div className="flex gap-3">
        {inquiry.artwork_primary_image_url ? (
          // eslint-disable-next-line @next/next/no-img-element
          <img
            src={inquiry.artwork_primary_image_url}
            alt=""
            className="w-12 h-12 object-cover bg-background shrink-0"
          />
        ) : (
          <div className="w-12 h-12 bg-background shrink-0" aria-hidden />
        )}
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline justify-between gap-2">
            <p
              className={clsx(
                "text-sm truncate",
                isUnread ? "font-medium" : "text-muted",
              )}
            >
              {isUnread && (
                <span
                  aria-label="unread"
                  className="inline-block w-1.5 h-1.5 rounded-full bg-foreground mr-2 align-middle"
                />
              )}
              {inquiry.from_name}
            </p>
            <span className="text-xs text-muted shrink-0">
              {formatRelative(latest)}
            </span>
          </div>
          <p className="text-xs text-muted truncate">
            {inquiry.artwork_title ?? "an artwork"}
          </p>
          <p
            className={clsx(
              "text-xs truncate mt-1",
              isUnread ? "text-foreground" : "text-muted",
            )}
          >
            {latestReply?.from_role === "artist" ? "You: " : ""}
            {snippet}
          </p>
        </div>
      </div>
    </Link>
  );
}

function InquiryThread({
  inquiry,
  onReplied,
}: {
  inquiry: StudioInquiry;
  onReplied: (inquiryId: string, reply: StudioInquiryReply) => void;
}) {
  const created = new Date(inquiry.created_at);
  const [formOpen, setFormOpen] = useState(false);
  const hasReplies = inquiry.replies.length > 0;

  return (
    <article className="border border-border bg-surface p-4">
      <div className="flex gap-4">
        {inquiry.artwork_primary_image_url ? (
          // eslint-disable-next-line @next/next/no-img-element
          <img
            src={inquiry.artwork_primary_image_url}
            alt=""
            className="w-20 h-20 object-cover bg-background flex-shrink-0"
          />
        ) : (
          <div
            className="w-20 h-20 bg-background flex-shrink-0"
            aria-hidden
          />
        )}
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline justify-between gap-3">
            <div className="min-w-0">
              <p className="text-sm">
                <span className="font-medium">{inquiry.from_name}</span>{" "}
                <a
                  href={`mailto:${inquiry.from_email}`}
                  className="text-muted hover:underline"
                >
                  &lt;{inquiry.from_email}&gt;
                </a>
              </p>
              <p className="text-xs text-muted mt-0.5">
                About{" "}
                <Link
                  href={`/artworks/${inquiry.artwork_id}`}
                  className="underline underline-offset-2 hover:text-foreground"
                >
                  {inquiry.artwork_title ?? "an artwork"}
                </Link>{" "}
                · {formatRelative(created)}
                {inquiry.budget_range ? (
                  <>
                    {" · "}
                    Budget:{" "}
                    <span className="text-foreground">
                      {inquiry.budget_range}
                    </span>
                  </>
                ) : null}
              </p>
            </div>
            <StatusBadge status={inquiry.status} />
          </div>
          <p className="mt-3 text-sm whitespace-pre-line">{inquiry.message}</p>
        </div>
      </div>

      {hasReplies && (
        <ReplyList replies={inquiry.replies} fromName={inquiry.from_name} />
      )}

      <div className="mt-3 pt-3 border-t border-border">
        {!formOpen ? (
          <button
            type="button"
            onClick={() => setFormOpen(true)}
            className="text-sm underline underline-offset-2 text-muted hover:text-foreground"
          >
            {hasReplies ? "Send another reply" : "Reply"}
          </button>
        ) : (
          <ReplyForm
            inquiryId={inquiry.id}
            onCancel={() => setFormOpen(false)}
            onSent={(reply) => {
              onReplied(inquiry.id, reply);
              setFormOpen(false);
            }}
          />
        )}
      </div>
    </article>
  );
}

function ReplyList({
  replies,
  fromName,
}: {
  replies: StudioInquiryReply[];
  fromName: string;
}) {
  return (
    <ul className="mt-3 pt-3 border-t border-border flex flex-col gap-2">
      {replies.map((r) => {
        // Inquirer replies are stitched back in from inbound email
        // (T-054); give them a left accent so the thread reads as a
        // back-and-forth rather than a list of the artist's own sends.
        const isArtist = r.from_role === "artist";
        return (
          <li
            key={r.id}
            className={clsx(
              "text-sm px-3 py-2",
              isArtist
                ? "bg-background"
                : "bg-surface border-l-2 border-foreground",
            )}
          >
            <p className="text-xs text-muted mb-1">
              {isArtist ? "You replied" : `${fromName} replied`}{" "}
              {formatRelative(new Date(r.created_at))}
              {isArtist && r.sent_at === null ? " · sending…" : ""}
            </p>
            <p className="whitespace-pre-line">{r.message}</p>
          </li>
        );
      })}
    </ul>
  );
}

function ReplyForm({
  inquiryId,
  onCancel,
  onSent,
}: {
  inquiryId: string;
  onCancel: () => void;
  onSent: (reply: StudioInquiryReply) => void;
}) {
  const [message, setMessage] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!message.trim() || loading) return;
    setLoading(true);
    setError(null);
    try {
      const res = await fetch(
        `/api/studio/inquiries/${encodeURIComponent(inquiryId)}/reply`,
        {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ message }),
        },
      );
      if (!res.ok) {
        const j = (await res.json().catch(() => ({}))) as {
          error?: string;
        };
        throw new Error(j.error ?? `HTTP ${res.status}`);
      }
      const reply = (await res.json()) as StudioInquiryReply;
      onSent(reply);
    } catch (e) {
      setError(
        toUserMessage(e, "Couldn't send the reply. Try again.", {
          surface: "studio-inquiry-reply",
        }),
      );
    } finally {
      setLoading(false);
    }
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-2">
      <label className="sr-only" htmlFor={`reply-${inquiryId}`}>
        Your reply
      </label>
      <textarea
        id={`reply-${inquiryId}`}
        value={message}
        onChange={(e) => setMessage(e.target.value)}
        rows={4}
        placeholder="Reply directly to the buyer — your message lands in their inbox."
        className="w-full border border-border bg-background p-2 text-sm font-sans"
        disabled={loading}
        autoFocus
      />
      <div className="flex items-center justify-between gap-2">
        {error ? (
          <p role="alert" className="text-xs text-red-600">
            {error}
          </p>
        ) : (
          <span className="text-xs text-muted">
            Goes to{" "}
            <span className="font-mono text-foreground">via email</span> ·
            they can reply directly to you.
          </span>
        )}
        <div className="flex gap-2">
          <button
            type="button"
            onClick={onCancel}
            disabled={loading}
            className="text-sm text-muted hover:text-foreground disabled:opacity-60"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={loading || !message.trim()}
            className="text-sm border border-foreground bg-foreground text-background px-3 py-1.5 disabled:opacity-60 disabled:cursor-progress"
          >
            {loading ? "Sending…" : "Send reply"}
          </button>
        </div>
      </div>
    </form>
  );
}

function StatusBadge({ status }: { status: StudioInquiry["status"] }) {
  if (status === "delivered") {
    return (
      <span
        className={clsx(
          "text-xs px-2 py-0.5 border border-border text-muted",
        )}
      >
        Delivered
      </span>
    );
  }
  return (
    <span className="text-xs px-2 py-0.5 border border-border bg-background">
      Awaiting verification
    </span>
  );
}

/** Latest activity timestamp (created_at OR newest reply's created_at)
 * as a number so sort() works cleanly. */
function latestActivity(inq: StudioInquiry): number {
  const created = new Date(inq.created_at).getTime();
  const latestReply = inq.replies[inq.replies.length - 1];
  if (!latestReply) return created;
  return Math.max(created, new Date(latestReply.created_at).getTime());
}

/**
 * Tiny "5m ago / 2h ago / 3d ago / Jan 4" formatter.
 */
function formatRelative(d: Date): string {
  const now = Date.now();
  const diff = Math.max(0, now - d.getTime());
  const minutes = Math.floor(diff / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d ago`;
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
