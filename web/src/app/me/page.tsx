/**
 * /me — temporary debug page. Calls /v1/me through `apiFetch` to confirm:
 *   1. Clerk session token is forwarded as Authorization: Bearer
 *   2. The Rust API verifies it against the JWKS
 *   3. First-sight users get upserted into our `users` table with their
 *      Clerk email fetched from Clerk's backend API
 *
 * Useful while wiring auth. Delete or repurpose once the studio surface
 * exists.
 */

import Link from "next/link";
import { TopNav } from "@/components/TopNav";

interface Me {
  id: string;
  clerk_user_id: string;
  email: string;
  is_admin: boolean;
}

async function callMe(): Promise<
  { ok: true; me: Me } | { ok: false; status: number; body: string }
> {
  const { cookies } = await import("next/headers");
  const { auth } = await import("@clerk/nextjs/server");
  const { ANON_COOKIE_NAME, verifyAnonId } = await import("@/lib/anonId");

  const headers = new Headers();
  try {
    const jar = await cookies();
    const raw = jar.get(ANON_COOKIE_NAME)?.value;
    if (raw) {
      const uuid = await verifyAnonId(raw);
      if (uuid) headers.set("X-Anonymous-Id", uuid);
    }
  } catch {}

  try {
    const { getToken } = await auth();
    const token = await getToken();
    if (token) headers.set("Authorization", `Bearer ${token}`);
  } catch {}

  const base = process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:9100";
  const res = await fetch(`${base}/v1/me`, { headers, cache: "no-store" });
  if (!res.ok) {
    return {
      ok: false,
      status: res.status,
      body: await res.text().catch(() => ""),
    };
  }
  return { ok: true, me: (await res.json()) as Me };
}

export default async function MePage() {
  const result = await callMe();

  return (
    <>
      <TopNav />
      <main className="flex-1 mx-auto w-full max-w-2xl px-6 py-16">
        <h1 className="font-serif text-3xl mb-6">Me (debug)</h1>

        {result.ok ? (
          <>
            <p className="text-sm text-muted mb-4">
              Authenticated. The chain (Clerk → Bearer header → JWKS verify →
              users upsert) is working.
            </p>
            <pre className="bg-surface border border-border p-4 text-xs overflow-x-auto">
              {JSON.stringify(result.me, null, 2)}
            </pre>
          </>
        ) : (
          <>
            <p className="text-sm mb-2">
              Not authenticated (or token rejected).{" "}
              <Link href="/sign-in" className="underline">
                Sign in
              </Link>{" "}
              and refresh.
            </p>
            <p className="text-xs text-muted mt-3">
              API responded {result.status}.
            </p>
            {result.body && (
              <pre className="mt-3 bg-surface border border-border p-3 text-xs overflow-x-auto">
                {result.body}
              </pre>
            )}
          </>
        )}
      </main>
    </>
  );
}
