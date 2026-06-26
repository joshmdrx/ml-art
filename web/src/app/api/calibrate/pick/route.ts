/**
 * `POST /api/calibrate/pick` — T-061 bridge.
 *
 * Browser → Next.js → api.wander.gallery. Same-origin so the
 * `anon_id` cookie reaches the api side — calibrator picks are
 * keyed on it for anonymous users so they fold into the user's
 * taste vector at sign-in via T-033 anon-merge.
 *
 * The api side (`api-search::calibrate::pick`) treats both anon and
 * signed-in callers; we just forward the body verbatim.
 */

import { NextResponse } from "next/server";

import { postCalibratePick } from "@/lib/api";
import { reportError } from "@/lib/reportError";

export async function POST(req: Request) {
  let body: unknown;
  try {
    body = await req.json();
  } catch {
    return NextResponse.json({ error: "invalid_json" }, { status: 400 });
  }

  try {
    const res = await postCalibratePick(body);
    return new NextResponse(null, { status: res.status });
  } catch (e) {
    reportError(e, { surface: "calibrate-bridge" });
    return NextResponse.json({ error: "calibrate_bridge_failed" }, { status: 502 });
  }
}
