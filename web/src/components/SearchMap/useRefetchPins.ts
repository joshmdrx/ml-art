"use client";

/**
 * Refetch the map's pin set against a given bbox + the current
 * non-bbox filters. Owns:
 *   - de-duping (skip if the bbox is unchanged since last fetch)
 *   - soft-handling 429 (don't `reportError`; show a status note)
 *   - loading + error state
 *
 * The hook keeps the latest filters in a ref so the stable `refetch`
 * function — captured once and registered on Mapbox's `moveend`
 * listener — always reads the freshest values rather than a stale
 * closure from mount time. Mount-time-only effect with stale-closure
 * traps was the previous SearchMap's biggest readability hazard.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import type { MapPin } from "@/lib/api";
import { searchMapClient } from "@/lib/searchMapClient";
import { reportError } from "@/lib/reportError";

export interface MapFilters {
  q?: string;
  medium?: string;
  location?: string;
  artist?: string;
  artist_ids?: string;
}

export interface RefetchPins {
  pins: MapPin[];
  loading: boolean;
  error: string | null;
  /** Trigger a refetch for `bbox` (no-op if it matches the last
   * successful fetch). Stable identity — safe to register on a
   * Mapbox listener once. */
  refetch: (bbox: string) => Promise<void>;
}

export function useRefetchPins(
  initial: MapPin[],
  filters: MapFilters
): RefetchPins {
  const [pins, setPins] = useState<MapPin[]>(initial);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // React's recommended "derived state" shape: shadow the prop in
  // another state slot and compare during render. When the prop
  // changes (city pivot click, Near me, back/forward → server
  // re-rendered with fresh `initial`), both updates land in the
  // same render — no useEffect, no cascading render, no setState-
  // in-effect lint complaint.
  //   https://react.dev/learn/you-might-not-need-an-effect#adjusting-some-state-when-a-prop-changes
  const [prevInitial, setPrevInitial] = useState(initial);
  if (prevInitial !== initial) {
    setPrevInitial(initial);
    setPins(initial);
  }

  // Refs that the stable refetch callback reads on every call. Lets
  // us register the listener once on mount but still pick up filter
  // changes between renders without re-registering.
  const filtersRef = useRef(filters);
  useEffect(() => {
    filtersRef.current = filters;
  }, [filters]);

  // Last-bbox dedup. Cleared on filter change so a same-bbox-but-
  // different-q request still goes through.
  const lastFetchedBboxRef = useRef<string | null>(null);
  useEffect(() => {
    lastFetchedBboxRef.current = null;
  }, [
    filters.q,
    filters.medium,
    filters.location,
    filters.artist,
    filters.artist_ids,
  ]);

  const refetch = useCallback(async (bbox: string) => {
    if (lastFetchedBboxRef.current === bbox) return;
    lastFetchedBboxRef.current = bbox;
    setLoading(true);
    try {
      const f = filtersRef.current;
      const next = await searchMapClient({
        q: f.q,
        medium: f.medium,
        location: f.location,
        artist: f.artist,
        artist_ids: f.artist_ids,
        bbox,
      });
      setPins(next);
      setError(null);
    } catch (e) {
      // 429 is our own rate limit — surface a friendly note rather
      // than reporting it. Clear the dedup so the next move retries.
      const rawMsg = e instanceof Error ? e.message : String(e);
      if (rawMsg.includes("429")) {
        setError("Slow down a moment — refreshing pins again shortly.");
        lastFetchedBboxRef.current = null;
      } else {
        reportError(e, { surface: "search-map-refetch" });
        setError("Couldn't refresh the map. Try moving again.");
      }
    } finally {
      setLoading(false);
    }
  }, []);

  return { pins, loading, error, refetch };
}
