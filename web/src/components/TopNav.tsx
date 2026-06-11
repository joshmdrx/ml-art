import Link from "next/link";
import { Show, UserButton } from "@clerk/nextjs";
import { SearchBar } from "./SearchBar";

/**
 * Persistent sticky top nav.
 * Logo (left) — Search (center, optional) — Auth (right).
 *
 * `hideSearch` should be true on the homepage, where the hero search is the
 * canonical entry — having both is visually noisy.
 */
export function TopNav({
  initialQuery = "",
  hideSearch = false,
}: {
  initialQuery?: string;
  hideSearch?: boolean;
}) {
  return (
    <header className="sticky top-0 z-30 bg-background/85 backdrop-blur border-b border-border">
      <div className="mx-auto max-w-screen-2xl px-6 h-14 flex items-center gap-6">
        <Link
          href="/"
          className="font-serif text-lg tracking-tight shrink-0"
          aria-label="Home"
        >
          Wander
        </Link>
        <div className="flex-1 flex justify-center">
          {!hideSearch && (
            <SearchBar size="nav" initialQuery={initialQuery} />
          )}
        </div>
        <nav className="shrink-0 flex items-center gap-4 text-sm">
          <Link
            href="/neighborhoods"
            className="text-muted hover:text-foreground"
          >
            Neighborhoods
          </Link>
          <Show when="signed-out">
            <Link
              href="/sign-in"
              className="text-muted hover:text-foreground"
            >
              Sign in
            </Link>
            <Link
              href="/sign-up"
              className="px-3 py-1.5 bg-foreground text-background hover:bg-foreground/90 transition-colors"
            >
              Sign up
            </Link>
          </Show>
          <Show when="signed-in">
            <Link
              href="/studio"
              className="text-muted hover:text-foreground"
            >
              Studio
            </Link>
            <Link
              href="/collections"
              className="text-muted hover:text-foreground"
            >
              Collections
            </Link>
            {/* `<UserButton>` is Clerk's pre-styled avatar + dropdown. We
                size it down to fit the nav row height. */}
            <UserButton
              appearance={{
                elements: {
                  avatarBox: "w-8 h-8",
                },
              }}
            />
          </Show>
        </nav>
      </div>
    </header>
  );
}
