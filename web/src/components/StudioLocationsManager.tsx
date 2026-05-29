"use client";

/**
 * T-038 G3 — "Where to see my work" CRUD on /studio/settings.
 *
 * Manages the artist's `artist_locations` rows: a list of gallery /
 * studio places a viewer can actually go to. Each row geocodes
 * asynchronously on the server (Mapbox); the UI surfaces a "Locating…"
 * label while lat/lng are still null, and a "Pin set" indicator once
 * the geocode lands.
 *
 * Refresh strategy: after a mutation we `router.refresh()` to re-fetch
 * the server-rendered list. If a row is still un-geocoded (a real
 * Mapbox call hasn't returned yet), we set a short interval to refresh
 * again. This avoids websockets / SSE; the rows are tiny so polling is
 * cheap. The poll stops the moment every row has a pin.
 */

import { useEffect, useState, useTransition, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import {
  createLocation,
  deleteLocation,
  patchLocation,
} from "@/app/actions/studio";
import type { StudioLocation } from "@/lib/api";
import { normalizeWebsiteUrl } from "@/lib/normalizeUrl";
import { reportError } from "@/lib/reportError";

interface Props {
  initial: StudioLocation[];
}

/**
 * Source-of-truth model: `initial` is the latest server snapshot (the
 * page revalidates after every mutation). We don't mirror it in local
 * state — `router.refresh()` after a successful action triggers a
 * re-render with the new prop, which is cheaper than syncing two
 * sources and keeps us out of the `set-state-in-effect` lint trap.
 *
 * Optimistic UX is bounded by how fast revalidate + refresh round-trip
 * (sub-100ms locally), which is acceptable for a low-frequency CRUD
 * surface — we're not optimizing the "add 20 galleries in a minute"
 * workflow.
 */
export function StudioLocationsManager({ initial }: Props) {
  const router = useRouter();
  const [showAdd, setShowAdd] = useState(false);

  // Poll while any row is still pre-geocode. Mapbox usually returns
  // inside 500ms but we set the cadence at 3s to keep the refresh
  // gentle and to give the (test-disabled) no-op path time to stamp
  // geocoded_at. Stops as soon as every row has a pin.
  useEffect(() => {
    const anyPending = initial.some((l) => l.lat == null);
    if (!anyPending) return;
    const id = setInterval(() => router.refresh(), 3_000);
    return () => clearInterval(id);
  }, [initial, router]);

  function handleCreated() {
    setShowAdd(false);
    router.refresh();
  }

  function handlePatched() {
    router.refresh();
  }

  function handleDeleted() {
    router.refresh();
  }

  return (
    <section className="mt-12 border-t border-border pt-8">
      <header className="flex items-center justify-between">
        <div>
          <h2 className="font-serif text-2xl tracking-tight">
            Where to see my work
          </h2>
          <p className="mt-1 text-sm text-muted">
            Galleries you&apos;re represented by, or your studio if you take
            visitors. Pins appear on your public profile and on the search
            map once geocoded.
          </p>
        </div>
        {!showAdd && (
          <button
            type="button"
            onClick={() => setShowAdd(true)}
            className="text-sm border border-border px-3 py-1.5 hover:bg-surface"
          >
            Add location
          </button>
        )}
      </header>

      {showAdd && (
        <AddLocationForm
          onCreated={handleCreated}
          onCancel={() => setShowAdd(false)}
        />
      )}

      {initial.length === 0 && !showAdd && (
        <p className="mt-6 text-sm text-muted">
          No locations yet. Add a gallery or your studio so viewers can find
          your work in person.
        </p>
      )}

      <ul className="mt-6 divide-y divide-border">
        {initial.map((loc) => (
          <LocationRow
            key={loc.id}
            location={loc}
            onPatched={handlePatched}
            onDeleted={handleDeleted}
          />
        ))}
      </ul>
    </section>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Add form (inline; expands above the list when "Add location" is clicked)
// ─────────────────────────────────────────────────────────────────────────────

function AddLocationForm({
  onCreated,
  onCancel,
}: {
  onCreated: () => void;
  onCancel: () => void;
}) {
  const [kind, setKind] = useState<"gallery" | "studio">("gallery");
  const [name, setName] = useState("");
  const [address, setAddress] = useState("");
  const [websiteUrl, setWebsiteUrl] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [isPending, startTransition] = useTransition();

  function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    startTransition(async () => {
      try {
        const normalizedUrl = normalizeWebsiteUrl(websiteUrl);
        await createLocation({
          kind,
          name: name.trim(),
          address: address.trim(),
          website_url: normalizedUrl ?? undefined,
        });
        onCreated();
      } catch (e) {
        reportError(e, { surface: "studio-locations-add" });
        setError(e instanceof Error ? e.message : "Couldn't add location");
      }
    });
  }

  return (
    <form
      onSubmit={onSubmit}
      className="mt-6 border border-border bg-surface p-4 space-y-3"
    >
      <div className="grid grid-cols-2 gap-3">
        <label className="block text-sm">
          <span className="block mb-1 text-muted">Kind</span>
          <select
            value={kind}
            onChange={(e) => setKind(e.target.value as "gallery" | "studio")}
            className="w-full border border-border bg-bg px-2 py-1.5"
          >
            <option value="gallery">Gallery</option>
            <option value="studio">Studio</option>
          </select>
        </label>
        <label className="block text-sm">
          <span className="block mb-1 text-muted">Name</span>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
            maxLength={200}
            placeholder="Foo Gallery"
            className="w-full border border-border bg-bg px-2 py-1.5"
          />
        </label>
      </div>
      <label className="block text-sm">
        <span className="block mb-1 text-muted">Address</span>
        <input
          type="text"
          value={address}
          onChange={(e) => setAddress(e.target.value)}
          required
          maxLength={500}
          placeholder="1 Example St, London EC1A 1AA"
          className="w-full border border-border bg-bg px-2 py-1.5"
        />
      </label>
      <label className="block text-sm">
        <span className="block mb-1 text-muted">Website (optional)</span>
        <input
          type="text"
          inputMode="url"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          value={websiteUrl}
          onChange={(e) => setWebsiteUrl(e.target.value)}
          maxLength={500}
          placeholder="foo-gallery.com"
          className="w-full border border-border bg-bg px-2 py-1.5"
        />
      </label>
      {error && <p className="text-sm text-red-600">{error}</p>}
      <div className="flex items-center justify-end gap-2 pt-1">
        <button
          type="button"
          onClick={onCancel}
          className="text-sm px-3 py-1.5 hover:bg-bg"
        >
          Cancel
        </button>
        <button
          type="submit"
          disabled={
            isPending || name.trim().length === 0 || address.trim().length === 0
          }
          className="text-sm px-3 py-1.5 bg-fg text-bg disabled:opacity-50"
        >
          {isPending ? "Adding…" : "Add"}
        </button>
      </div>
    </form>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Single row — read mode + inline edit mode
// ─────────────────────────────────────────────────────────────────────────────

function LocationRow({
  location,
  onPatched,
  onDeleted,
}: {
  location: StudioLocation;
  onPatched: () => void;
  onDeleted: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [isPending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);

  function onDelete() {
    setError(null);
    startTransition(async () => {
      try {
        await deleteLocation(location.id);
        onDeleted();
      } catch (e) {
        reportError(e, { surface: "studio-locations-delete" });
        setError(e instanceof Error ? e.message : "Couldn't delete");
      }
    });
  }

  if (editing) {
    return (
      <EditLocationForm
        location={location}
        onCancel={() => setEditing(false)}
        onSaved={() => {
          onPatched();
          setEditing(false);
        }}
      />
    );
  }

  const pinStatus =
    location.lat != null && location.lng != null ? (
      <span className="text-xs text-muted">
        Pin set{location.city ? ` · ${location.city}` : ""}
      </span>
    ) : (
      <span className="text-xs text-amber-700">Locating…</span>
    );

  return (
    <li className="py-4 flex items-start justify-between gap-4">
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-[10px] uppercase tracking-wider text-muted">
            {location.kind}
          </span>
          {pinStatus}
        </div>
        <p className="mt-1 font-medium">{location.name}</p>
        <p className="text-sm text-muted">{location.address}</p>
        {location.website_url && (
          <a
            href={location.website_url}
            target="_blank"
            rel="noopener noreferrer"
            className="text-sm text-muted underline break-all"
          >
            {location.website_url}
          </a>
        )}
        {error && <p className="mt-2 text-sm text-red-600">{error}</p>}
      </div>
      <div className="flex items-center gap-2 shrink-0">
        <button
          type="button"
          onClick={() => setEditing(true)}
          className="text-xs border border-border px-2 py-1 hover:bg-surface"
        >
          Edit
        </button>
        {confirming ? (
          <>
            <button
              type="button"
              onClick={onDelete}
              disabled={isPending}
              className="text-xs border border-red-600 text-red-600 px-2 py-1 disabled:opacity-50"
            >
              {isPending ? "Deleting…" : "Confirm"}
            </button>
            <button
              type="button"
              onClick={() => setConfirming(false)}
              className="text-xs px-2 py-1 hover:bg-surface"
            >
              Cancel
            </button>
          </>
        ) : (
          <button
            type="button"
            onClick={() => setConfirming(true)}
            className="text-xs px-2 py-1 hover:bg-surface text-muted"
          >
            Delete
          </button>
        )}
      </div>
    </li>
  );
}

function EditLocationForm({
  location,
  onCancel,
  onSaved,
}: {
  location: StudioLocation;
  onCancel: () => void;
  onSaved: () => void;
}) {
  const [kind, setKind] = useState<"gallery" | "studio">(location.kind);
  const [name, setName] = useState(location.name);
  const [address, setAddress] = useState(location.address);
  const [websiteUrl, setWebsiteUrl] = useState(location.website_url ?? "");
  const [error, setError] = useState<string | null>(null);
  const [isPending, startTransition] = useTransition();

  function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);

    // Diff against the initial values — only PATCH what actually changed.
    const patch: Parameters<typeof patchLocation>[1] = {};
    if (kind !== location.kind) patch.kind = kind;
    if (name.trim() !== location.name) patch.name = name.trim();
    if (address.trim() !== location.address) patch.address = address.trim();
    // `normalizeWebsiteUrl` returns null for empty input and prepends
    // `https://` if the user typed a bare hostname. Compare normalized
    // forms so editing "guitardojo.app" → "https://guitardojo.app"
    // doesn't mark the field dirty on every render.
    const newUrl = normalizeWebsiteUrl(websiteUrl);
    const oldUrl = location.website_url ?? null;
    if (newUrl !== oldUrl) {
      patch.website_url = newUrl;
    }
    if (Object.keys(patch).length === 0) {
      onCancel();
      return;
    }

    startTransition(async () => {
      try {
        await patchLocation(location.id, patch);
        onSaved();
      } catch (e) {
        reportError(e, { surface: "studio-locations-edit" });
        setError(e instanceof Error ? e.message : "Couldn't save");
      }
    });
  }

  return (
    <li className="py-4">
      <form
        onSubmit={onSubmit}
        className="border border-border bg-surface p-4 space-y-3"
      >
        <div className="grid grid-cols-2 gap-3">
          <label className="block text-sm">
            <span className="block mb-1 text-muted">Kind</span>
            <select
              value={kind}
              onChange={(e) =>
                setKind(e.target.value as "gallery" | "studio")
              }
              className="w-full border border-border bg-bg px-2 py-1.5"
            >
              <option value="gallery">Gallery</option>
              <option value="studio">Studio</option>
            </select>
          </label>
          <label className="block text-sm">
            <span className="block mb-1 text-muted">Name</span>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
              maxLength={200}
              className="w-full border border-border bg-bg px-2 py-1.5"
            />
          </label>
        </div>
        <label className="block text-sm">
          <span className="block mb-1 text-muted">Address</span>
          <input
            type="text"
            value={address}
            onChange={(e) => setAddress(e.target.value)}
            required
            maxLength={500}
            className="w-full border border-border bg-bg px-2 py-1.5"
          />
          <p className="mt-1 text-xs text-muted">
            Changing the address re-geocodes the pin.
          </p>
        </label>
        <label className="block text-sm">
          <span className="block mb-1 text-muted">Website (optional)</span>
          <input
            type="text"
            inputMode="url"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            value={websiteUrl}
            onChange={(e) => setWebsiteUrl(e.target.value)}
            maxLength={500}
            placeholder="foo-gallery.com"
            className="w-full border border-border bg-bg px-2 py-1.5"
          />
        </label>
        {error && <p className="text-sm text-red-600">{error}</p>}
        <div className="flex items-center justify-end gap-2 pt-1">
          <button
            type="button"
            onClick={onCancel}
            className="text-sm px-3 py-1.5 hover:bg-bg"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={
              isPending ||
              name.trim().length === 0 ||
              address.trim().length === 0
            }
            className="text-sm px-3 py-1.5 bg-fg text-bg disabled:opacity-50"
          >
            {isPending ? "Saving…" : "Save"}
          </button>
        </div>
      </form>
    </li>
  );
}
