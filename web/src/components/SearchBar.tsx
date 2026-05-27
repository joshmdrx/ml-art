"use client";

/**
 * Search input. Two visual sizes:
 *   - `hero`: large centered (homepage)
 *   - `nav`:  smaller inline (top nav, every other page)
 *
 * On submit, navigates to /search?q=...
 */

import { useRouter } from "next/navigation";
import { useState, type FormEvent } from "react";
import { clsx } from "clsx";

type Size = "hero" | "nav";

export function SearchBar({
  size,
  initialQuery = "",
}: {
  size: Size;
  initialQuery?: string;
}) {
  const router = useRouter();
  const [value, setValue] = useState(initialQuery);

  function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const q = value.trim();
    router.push(q ? `/search?q=${encodeURIComponent(q)}` : "/search");
  }

  const placeholder =
    size === "hero"
      ? "Search artworks, artists, or drop an image."
      : "Search";

  return (
    <form
      onSubmit={onSubmit}
      className={clsx(
        "w-full",
        size === "hero" ? "max-w-2xl" : "max-w-md"
      )}
    >
      <input
        type="text"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder={placeholder}
        aria-label="Search"
        className={clsx(
          "w-full bg-surface border border-border focus:outline-none focus:border-foreground transition-colors",
          size === "hero"
            ? "py-4 px-5 text-lg"
            : "py-2 px-3 text-sm"
        )}
      />
    </form>
  );
}
