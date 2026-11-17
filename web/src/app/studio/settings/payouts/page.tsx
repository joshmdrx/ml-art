import type { Metadata } from "next";
import Link from "next/link";
import { redirect } from "next/navigation";
import { auth } from "@clerk/nextjs/server";
import { EnableSalesButton } from "@/components/studio/EnableSalesButton";
import { getPayoutStatus } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export const metadata: Metadata = { title: "Payouts" };

/**
 * `/studio/settings/payouts` — enable direct sales via Stripe Connect
 * (M-01/M-06). Also the return URL from Stripe's hosted onboarding, so
 * we re-read status on load; `account.updated` may still be in flight.
 * In the started-but-not-live state the API's `details_due` (a live Stripe
 * requirements check) picks between "finish setting up" and "Stripe is
 * reviewing"; when it's null we fall back to copy covering both.
 */
export default async function PayoutsPage() {
  const { userId } = await auth();
  if (!userId) {
    redirect(
      "/sign-in?redirect_url=" +
        encodeURIComponent("/studio/settings/payouts")
    );
  }

  const status = await getPayoutStatus().catch((e) => {
    reportError(e, { surface: "payouts-settings" });
    return null;
  });

  const live = status?.charges_enabled ?? false;
  const started = status?.onboarding_started ?? false;

  return (
    <div className="flex-1 px-6 py-8 lg:py-10 max-w-2xl">
      <Link
        href="/studio/settings"
        className="text-xs text-muted hover:text-foreground"
      >
        ← Settings
      </Link>
      <h1 className="font-serif text-3xl tracking-tight mt-4">Payouts</h1>
      <p className="mt-2 text-sm text-muted">
        Let collectors buy your work directly through Wander. We use Stripe
        to handle payments and pay out to your bank; Wander takes a 15%
        commission on each sale.
      </p>

      <section className="mt-8 border border-border bg-surface p-5">
        {live ? (
          <>
            <p className="text-sm font-medium">✓ Direct sales are live</p>
            <p className="text-sm text-muted mt-1">
              Your eligible works now show a Buy button.
              {status?.payouts_enabled === false &&
                " Payouts to your bank are still being verified by Stripe."}
            </p>
            <EnableSalesButton started={started} live={live} />
          </>
        ) : started ? (
          <>
            {status?.details_due ? (
              <>
                <p className="text-sm font-medium">Finish setting up</p>
                <p className="text-sm text-muted mt-1">
                  Stripe still needs a few details from you before your works
                  can be bought. Continue where you left off — it only asks
                  for what&apos;s missing.
                </p>
              </>
            ) : status?.details_due === false ? (
              <>
                <p className="text-sm font-medium">
                  Stripe is reviewing your details
                </p>
                <p className="text-sm text-muted mt-1">
                  You&apos;ve submitted everything. This usually takes a few
                  minutes; refresh this page to check.
                </p>
              </>
            ) : (
              <>
                <p className="text-sm font-medium">Setup in progress</p>
                <p className="text-sm text-muted mt-1">
                  If you didn&apos;t finish with Stripe, continue where you
                  left off. Otherwise Stripe may still be reviewing your
                  details — this usually takes a few minutes.
                </p>
              </>
            )}
            <EnableSalesButton started={started} live={live} />
          </>
        ) : (
          <>
            <p className="text-sm font-medium">Set up direct sales</p>
            <p className="text-sm text-muted mt-1">
              You&apos;ll be taken to Stripe to add your bank details and
              verify your identity (about 5 minutes).
            </p>
            <EnableSalesButton started={started} live={live} />
          </>
        )}
      </section>
    </div>
  );
}
