/**
 * Step progress strip for `/onboarding` (T-012 Phase 1).
 *
 * Server-rendered, no interactivity — current step is determined by
 * the page's URL and passed in as a prop. Each step is a labeled chip;
 * completed steps are filled in, the current one is outlined, future
 * steps are muted. The artist can navigate back via the chip links
 * (forward steps are not clickable when not yet completed).
 */

import Link from "next/link";

export type OnboardingStep =
  | "identity"
  | "profile"
  | "artworks"
  | "locations"
  | "review";

export const STEP_ORDER: OnboardingStep[] = [
  "identity",
  "profile",
  "artworks",
  "locations",
  "review",
];

const STEP_LABEL: Record<OnboardingStep, string> = {
  identity: "Identity",
  profile: "Profile",
  artworks: "Artworks",
  locations: "Where to see",
  review: "Review",
};

interface Props {
  current: OnboardingStep;
  /** Furthest step the artist has reached. Steps beyond this index
   * are muted and not clickable. */
  furthest: OnboardingStep;
}

export function StepNav({ current, furthest }: Props) {
  const furthestIndex = STEP_ORDER.indexOf(furthest);
  const currentIndex = STEP_ORDER.indexOf(current);

  return (
    <ol
      className="mb-8 flex flex-wrap items-center gap-2 text-xs"
      aria-label="Onboarding progress"
    >
      {STEP_ORDER.map((step, idx) => {
        const isCurrent = idx === currentIndex;
        const isPast = idx < currentIndex;
        const isReachable = idx <= furthestIndex;
        const label = STEP_LABEL[step];
        const numbered = `${idx + 1}. ${label}`;

        const baseClass =
          "inline-flex items-center px-3 py-1 border border-border";
        const tone = isCurrent
          ? "bg-fg text-bg"
          : isPast
            ? "bg-surface"
            : "text-muted";

        if (isReachable && !isCurrent) {
          return (
            <li key={step}>
              <Link
                href={`/onboarding?step=${step}`}
                className={`${baseClass} ${tone} hover:bg-fg/10`}
                aria-current={isCurrent ? "step" : undefined}
              >
                {numbered}
              </Link>
            </li>
          );
        }
        return (
          <li
            key={step}
            className={`${baseClass} ${tone}`}
            aria-current={isCurrent ? "step" : undefined}
          >
            {numbered}
          </li>
        );
      })}
    </ol>
  );
}
