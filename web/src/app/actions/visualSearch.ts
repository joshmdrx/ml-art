"use server";

/**
 * Server actions for visual search. The upload action runs server-side
 * so the Clerk Bearer + anon-id headers get forwarded to the API
 * without touching the browser — same pattern as
 * `actions/collections.ts` and `actions/studio.ts`.
 */

import { redirect } from "next/navigation";
import { uploadImageForSearch } from "@/lib/api";
import { reportError } from "@/lib/reportError";

/**
 * Upload an image and redirect to the visual-search results page.
 * Called from `<form action={…}>` so the browser hands us a real
 * `FormData` with the file already buffered.
 *
 * Errors don't return — they redirect to `/search?upload_error=…` so
 * the page can surface a friendly state without a separate error
 * surface (the search page only needs to know "the search didn't fire
 * because the upload failed").
 */
export async function uploadAndStartVisualSearch(
  formData: FormData
): Promise<void> {
  const file = formData.get("image");
  if (!(file instanceof File)) {
    redirect("/search?upload_error=" + encodeURIComponent("no file provided"));
  }
  if (file.size === 0) {
    redirect("/search?upload_error=" + encodeURIComponent("file is empty"));
  }

  let uploadId: string;
  try {
    const buf = new Uint8Array(await file.arrayBuffer());
    const ack = await uploadImageForSearch({
      name: file.name || "upload.bin",
      type: file.type || "application/octet-stream",
      bytes: buf,
    });
    uploadId = ack.upload_id;
  } catch (err) {
    reportError(err, { surface: "visual-search-upload" });
    // Pass a stable error code, not the raw message — the search page
    // maps it to friendly copy. Keeps server-component error verbiage
    // out of URLs (and out of the user's address bar).
    redirect("/search?upload_error=1");
  }

  redirect(`/search?image_upload_id=${encodeURIComponent(uploadId)}`);
}
