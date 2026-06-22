"use client";

/**
 * Client side of the studio inquiry inbox (T-011 Phase 4 + 4b).
 *
 * The page above us is a server component that loads the initial list
 * + filter chrome; this owns:
 *
 *   - per-card reply form state (open / message / loading / error)
 *   - optimistic append of new replies
 *   - auto-mark-as-read on first render for whatever was unread
 *
 * Why a client component for the whole list (rather than just the
 * form): inquiries arrive ordered newest-first and the user expects
 * the optimistic append to stay put. Re-fetching the server payload
 * on every reply would re-order and lose the form-focus we just
 * scrolled to.
 */

import { useEffect, useRef, useState } from "react";
import Link from "next/link";
import clsx from "clsx";

import type { StudioInquiry, StudioInquiryReply } from "@/lib/api";
import { toUserMessage } from "@/lib/reportError";

interface Props {
  initialItems: StudioInquiry[];
}

export function InquiryInbox({ initialItems }: Props) {
  // Local copy so optimistic reply append doesn't fight the server
  // payload. Re-syncs when the server pushes a fresh list (filter
  // change in the URL).
  const [prevServerItems, setPrevServerItems] = useState(initialItems);
  const [items, setItems] = useState(initialItems);
  if (prevServerItems !== initialItems) {
    setPrevServerItems(initialItems);
    setItems(initialItems);
  }

  // Auto-mark-as-read for whatever was unread when the page loaded.
  // Fires once per `initialItems` identity — a filter change pushes a
  // new array so we'll mark the new set too. Errors are swallowed
  // (the user doesn't need to act on a failed mark-read).
  const markedRef = useRef<Set<StudioInquiry[]>>(new Set());
  useEffect(() => {
    if (markedRef.current.has(initialItems)) return;
    markedRef.current.add(initialItems);
    const unreadIds = initialItems
      .filter((i) => i.read_at === null)
      .map((i) => i.id);
    if (unreadIds.length === 0) return;
    fetch("/api/studio/inquiries/read", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ ids: unreadIds }),
    }).catch(() => {
      /* best-effort — silent */
    });
  }, [initialItems]);

  function onReplied(inquiryId: string, reply: StudioInquiryReply) {
    setItems((prev) =>
      prev.map((i) =>
        i.id === inquiryId
          ? { ...i, replies: [...i.replies, reply] }
          : i,
      ),
    );
  }

  return (
    <ul className="flex flex-col gap-3">
      {items.map((inq) => (
        <li key={inq.id}>
          <InquiryCard inquiry={inq} onReplied={onReplied} />
        </li>
      ))}
    </ul>
  );
}

function InquiryCard({
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
            Goes to {/* avoid quoting the email here so screen readers don't read the angle brackets */}
            <span className="font-mono text-foreground">via email</span> · they can reply directly to you.
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

/**
 * Tiny "5m ago / 2h ago / 3d ago / Jan 4" formatter. Same logic as
 * the server-rendered page used to inline; lives here so the card
 * doesn't need a server roundtrip to format timestamps for its
 * own optimistically-appended replies.
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
