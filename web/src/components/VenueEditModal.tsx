"use client";

/**
 * T-081.2 — venue edit modal.
 *
 * Two tabs:
 *   - Details: form fields (name, kind, about, address, website,
 *     instagram, opening info)
 *   - Artworks: existing invitations (pending / accepted / declined)
 *     + an invite-by-id input. The venue owner doesn't have direct
 *     visibility into the global artwork catalogue in v1; pasting an
 *     artwork URL or UUID is the affordance. Search-to-invite is a
 *     deferred follow-up.
 *
 * URL-driven lifecycle per `docs/ui-patterns.md`. The parent owns
 * `target` / `tab` / `onTabChange` via search params.
 */

import { useEffect, useState, useTransition } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { clsx } from "clsx";
import { toast } from "sonner";
import { useConfirm } from "@/components/ui/ConfirmDialog";
import { FieldError } from "@/components/ui/FieldError";
import {
  createVenue,
  loadVenueArtworks,
  removeVenue,
  setVenueArtworks,
  updateVenue,
} from "@/app/actions/venues";
import { toUserMessage } from "@/lib/reportError";
import type { Venue, VenueArtworkRow, VenueKind } from "@/lib/api";

type Tab = "details" | "artworks";

const KIND_OPTIONS: Array<{ value: VenueKind; label: string }> = [
  { value: "gallery", label: "Gallery" },
  { value: "shop", label: "Shop" },
  { value: "studio_collective", label: "Studio collective" },
  { value: "cafe_collab", label: "Café / collab space" },
  { value: "other", label: "Other" },
];

interface Props {
  open: boolean;
  target: Venue | "new" | null;
  tab: Tab;
  onTabChange: (next: Tab) => void;
  onSaved: (v: Venue) => void;
  onDeleted: (id: string) => void;
  onClose: () => void;
}

export function VenueEditModal({
  open,
  target,
  tab,
  onTabChange,
  onSaved,
  onDeleted,
  onClose,
}: Props) {
  const isNew = target === "new";
  const existing: Venue | null = target && target !== "new" ? target : null;

  return (
    <Dialog.Root open={open} onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-foreground/30 backdrop-blur-sm z-40" />
        <Dialog.Content
          className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-50 w-[92vw] max-w-2xl max-h-[90vh] overflow-y-auto bg-surface border border-border p-6 shadow-2xl"
          aria-describedby={undefined}
        >
          <Dialog.Title className="font-serif text-2xl mb-1">
            {isNew ? "New venue" : existing?.name ?? "Edit venue"}
          </Dialog.Title>
          {existing && (
            <p className="text-xs text-muted mb-4">
              /{existing.slug} ·{" "}
              {existing.status === "pending_review"
                ? "Pending admin review"
                : existing.status === "active"
                  ? "Public"
                  : existing.status === "paused"
                    ? "Paused"
                    : "Declined"}
            </p>
          )}

          {/* Tabs — hidden on create until a venue exists. */}
          {existing && (
            <nav className="mb-6 flex gap-2" aria-label="Venue sections">
              {(["details", "artworks"] as const).map((t) => (
                <button
                  key={t}
                  type="button"
                  onClick={() => onTabChange(t)}
                  className={clsx(
                    "px-3 py-1.5 text-sm border transition-colors",
                    tab === t
                      ? "border-foreground bg-foreground text-background"
                      : "border-border hover:bg-background",
                  )}
                >
                  {t === "details" ? "Details" : "Artworks"}
                </button>
              ))}
            </nav>
          )}

          {tab === "details" || !existing ? (
            <DetailsTab
              existing={existing}
              onSaved={onSaved}
              onDeleted={onDeleted}
            />
          ) : (
            <ArtworksTab venueId={existing.id} />
          )}

          <Dialog.Close asChild>
            <button
              type="button"
              aria-label="Close"
              className="absolute top-3 right-3 text-muted hover:text-foreground"
            >
              ×
            </button>
          </Dialog.Close>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Details tab
// ─────────────────────────────────────────────────────────────────────────────

function DetailsTab({
  existing,
  onSaved,
  onDeleted,
}: {
  existing: Venue | null;
  onSaved: (v: Venue) => void;
  /** UI-side hook fired after the API delete succeeds — parent
   * removes the venue from local state + closes the modal. */
  onDeleted: (id: string) => void;
}) {
  const confirm = useConfirm();
  const [name, setName] = useState(existing?.name ?? "");
  const [kind, setKind] = useState<VenueKind>(existing?.kind ?? "gallery");
  const [about, setAbout] = useState(existing?.about ?? "");
  const [address, setAddress] = useState(existing?.address ?? "");
  const [website, setWebsite] = useState(existing?.website_url ?? "");
  const [instagram, setInstagram] = useState(existing?.instagram_handle ?? "");
  const [opening, setOpening] = useState(existing?.opening_info ?? "");
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [isPending, startTransition] = useTransition();

  // Re-init form state when the target venue changes (parent URL
  // flip). Same load-from-external-store handshake as
  // SeriesEditModal — the set-state-in-effect lint flags this
  // generically but in this load-on-open pattern it's the intent.
  /* eslint-disable react-hooks/set-state-in-effect */
  useEffect(() => {
    setName(existing?.name ?? "");
    setKind(existing?.kind ?? "gallery");
    setAbout(existing?.about ?? "");
    setAddress(existing?.address ?? "");
    setWebsite(existing?.website_url ?? "");
    setInstagram(existing?.instagram_handle ?? "");
    setOpening(existing?.opening_info ?? "");
    setErrors({});
  }, [existing]);
  /* eslint-enable react-hooks/set-state-in-effect */

  function submit() {
    setErrors({});
    if (!name.trim()) {
      setErrors({ name: "Name is required." });
      return;
    }
    startTransition(async () => {
      try {
        const body = {
          name: name.trim(),
          kind,
          about: about.trim() || undefined,
          address: address.trim() || undefined,
          website_url: website.trim() || undefined,
          instagram_handle: instagram.trim() || undefined,
          opening_info: opening.trim() || undefined,
        };
        const result = existing
          ? await updateVenue(existing.id, body)
          : await createVenue(body);
        toast.success(existing ? "Venue updated" : "Venue created");
        onSaved(result);
      } catch (e) {
        toast.error(
          toUserMessage(e, "Couldn't save the venue. Check the form and try again.", {
            surface: "venue-edit-modal",
          }),
        );
      }
    });
  }

  async function doDelete() {
    if (!existing) return;
    const ok = await confirm({
      title: `Delete "${existing.name}"?`,
      description:
        "Soft-delete: the row stays in the database for audit, but public surfaces stop showing it immediately. Invitations stay on the artwork side until the artwork or venue is hard-deleted.",
      confirmLabel: "Delete venue",
      destructive: true,
    });
    if (!ok) return;
    try {
      await removeVenue(existing.id);
      toast.success("Venue deleted");
      onDeleted(existing.id);
    } catch (e) {
      toast.error(
        toUserMessage(e, "Couldn't delete the venue.", {
          surface: "venue-edit-modal",
          action: "delete",
        }),
      );
    }
  }

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
      className="space-y-3"
    >
      <label className="block">
        <span className="block text-xs text-muted mb-1">Name</span>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          maxLength={200}
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
        />
        <FieldError message={errors.name} />
      </label>

      <label className="block">
        <span className="block text-xs text-muted mb-1">Kind</span>
        <select
          value={kind}
          onChange={(e) => setKind(e.target.value as VenueKind)}
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
        >
          {KIND_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      </label>

      <label className="block">
        <span className="block text-xs text-muted mb-1">About</span>
        <textarea
          value={about}
          onChange={(e) => setAbout(e.target.value)}
          maxLength={4000}
          rows={3}
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground resize-y"
        />
      </label>

      <label className="block">
        <span className="block text-xs text-muted mb-1">Address</span>
        <input
          type="text"
          value={address}
          onChange={(e) => setAddress(e.target.value)}
          maxLength={500}
          placeholder="1 Test St, London EC1A 1AA"
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
        />
        <span className="block text-xs text-muted mt-1">
          We&apos;ll geocode this to a map pin in the background.
        </span>
      </label>

      <div className="grid grid-cols-2 gap-3">
        <label className="block">
          <span className="block text-xs text-muted mb-1">Website</span>
          <input
            type="url"
            value={website}
            onChange={(e) => setWebsite(e.target.value)}
            placeholder="https://"
            className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
          />
        </label>
        <label className="block">
          <span className="block text-xs text-muted mb-1">Instagram</span>
          <input
            type="text"
            value={instagram}
            onChange={(e) => setInstagram(e.target.value)}
            placeholder="@yourvenue"
            className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
          />
        </label>
      </div>

      <label className="block">
        <span className="block text-xs text-muted mb-1">Opening info</span>
        <input
          type="text"
          value={opening}
          onChange={(e) => setOpening(e.target.value)}
          maxLength={500}
          placeholder="Tue–Sat 11–6"
          className="w-full bg-background border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
        />
      </label>

      <div className="pt-3 flex items-center gap-2 justify-between">
        {existing && (
          <button
            type="button"
            onClick={doDelete}
            className="px-3 py-2 text-sm text-muted hover:text-foreground"
          >
            Delete venue
          </button>
        )}
        <button
          type="submit"
          disabled={isPending}
          className="ml-auto px-4 py-2 text-sm bg-foreground text-background disabled:opacity-40"
        >
          {isPending ? "Saving…" : existing ? "Save changes" : "Create venue"}
        </button>
      </div>
    </form>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Artworks tab
// ─────────────────────────────────────────────────────────────────────────────

function ArtworksTab({ venueId }: { venueId: string }) {
  const [rows, setRows] = useState<VenueArtworkRow[] | null>(null);
  const [inviteId, setInviteId] = useState("");
  const [inviteErr, setInviteErr] = useState<string | undefined>(undefined);
  const [isPending, startTransition] = useTransition();

  useEffect(() => {
    let cancelled = false;
    loadVenueArtworks(venueId)
      .then((r) => {
        if (!cancelled) setRows(r);
      })
      .catch(() => {
        if (!cancelled) setRows([]);
      });
    return () => {
      cancelled = true;
    };
  }, [venueId]);

  function invite() {
    setInviteErr(undefined);
    // Allow pasting an artwork URL like /artworks/<uuid>; extract the
    // trailing UUID if present.
    const raw = inviteId.trim();
    const id = raw.split("/").pop()?.trim() ?? "";
    if (!/^[0-9a-fA-F-]{36}$/.test(id)) {
      setInviteErr("Paste an artwork ID or URL.");
      return;
    }
    startTransition(async () => {
      try {
        const next = [...(rows ?? []).map((r) => r.artwork_id), id];
        // Use the diff-helper from the action — handles both invite +
        // uninvite. Here we only ever add, so the diff is a single
        // invite call.
        await setVenueArtworks(venueId, next);
        const refreshed = await loadVenueArtworks(venueId);
        setRows(refreshed);
        setInviteId("");
        toast.success("Invited");
      } catch (e) {
        setInviteErr(
          toUserMessage(e, "Couldn't invite this artwork.", {
            surface: "venue-edit-modal",
            artwork_id: id,
          }),
        );
      }
    });
  }

  function uninvite(artworkId: string) {
    startTransition(async () => {
      try {
        const next = (rows ?? [])
          .map((r) => r.artwork_id)
          .filter((id) => id !== artworkId);
        await setVenueArtworks(venueId, next);
        const refreshed = await loadVenueArtworks(venueId);
        setRows(refreshed);
        toast.success("Uninvited");
      } catch (e) {
        toast.error(
          toUserMessage(e, "Couldn't uninvite this artwork.", {
            surface: "venue-edit-modal",
            artwork_id: artworkId,
          }),
        );
      }
    });
  }

  return (
    <div className="space-y-4">
      <div className="border border-border bg-background p-3">
        <p className="text-xs text-muted mb-2">
          Invite an artwork by pasting its URL or ID. The artist
          accepts or declines — only accepted artworks show on your
          public venue page.
        </p>
        <div className="flex items-center gap-2">
          <input
            type="text"
            value={inviteId}
            onChange={(e) => setInviteId(e.target.value)}
            placeholder="/artworks/00000000-… or just the UUID"
            className="flex-1 bg-surface border border-border px-3 py-2 text-sm focus:outline-none focus:border-foreground"
          />
          <button
            type="button"
            onClick={invite}
            disabled={isPending}
            className="px-3 py-2 text-sm border border-foreground bg-foreground text-background disabled:opacity-40"
          >
            Invite
          </button>
        </div>
        <FieldError message={inviteErr} />
      </div>

      {rows === null ? (
        <p className="text-sm text-muted">Loading…</p>
      ) : rows.length === 0 ? (
        <p className="text-sm text-muted">
          No invitations yet. Add the first artwork above.
        </p>
      ) : (
        <ul className="divide-y divide-border border border-border">
          {rows.map((r) => (
            <li
              key={r.artwork_id}
              className="p-3 flex items-center justify-between gap-3"
            >
              <div className="min-w-0">
                <p className="text-sm line-clamp-1">
                  {r.artwork_title ?? <em className="text-muted">untitled</em>}
                </p>
                <p className="text-xs text-muted">
                  by {r.artist_display_name} · /{r.artist_slug}
                </p>
              </div>
              <div className="flex items-center gap-3 shrink-0">
                <StatusPill status={r.status} />
                <button
                  type="button"
                  onClick={() => uninvite(r.artwork_id)}
                  disabled={isPending}
                  className="text-xs text-muted hover:text-foreground underline underline-offset-2 disabled:opacity-40"
                >
                  Uninvite
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function StatusPill({ status }: { status: VenueArtworkRow["status"] }) {
  return (
    <span
      className={clsx(
        "text-[10px] tracking-wide uppercase px-1.5 py-0.5 border",
        status === "accepted"
          ? "border-foreground"
          : "border-border text-muted",
      )}
    >
      {status}
    </span>
  );
}
