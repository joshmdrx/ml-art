"use client";

/**
 * T-083.3 — one rejected image in `/admin/images`.
 *
 * Renders a thumbnail (rendered at 50% opacity to reinforce that the
 * image is hidden on public surfaces) + the auto-moderator's reason
 * code + an Override button.
 *
 * Override is destructive in the "publishes content that was hidden"
 * sense, so it goes through `useConfirm()` per docs/ui-patterns.md.
 */

import { useTransition } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { useConfirm } from "@/components/ui/ConfirmDialog";
import { overrideImageRejection } from "@/app/actions/admin";
import { toUserMessage } from "@/lib/reportError";
import type { AdminImageItem } from "@/lib/api";

export function AdminImageRow({ image }: { image: AdminImageItem }) {
  const router = useRouter();
  const confirm = useConfirm();
  const [isPending, startTransition] = useTransition();

  async function onOverride() {
    const ok = await confirm({
      title: `Override moderation on this image?`,
      description: `Flips moderation_status from "rejected" to "approved", clears the auto-mod reason. The image will appear on public surfaces immediately.`,
      confirmLabel: "Override (approve)",
      destructive: true,
    });
    if (!ok) return;
    startTransition(async () => {
      try {
        await overrideImageRejection(image.id);
        toast.success(`Approved: ${image.artwork_title ?? image.s3_key}`);
        router.refresh();
      } catch (e) {
        toast.error(
          toUserMessage(e, "Couldn't override this image.", {
            surface: "admin-images",
            image_id: image.id,
          }),
        );
      }
    });
  }

  return (
    <li className="border border-border bg-surface overflow-hidden">
      <div className="aspect-square w-full bg-background overflow-hidden relative">
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img
          src={image.url}
          alt={image.artwork_title ?? "rejected image"}
          className="w-full h-full object-contain opacity-50 grayscale"
          loading="lazy"
        />
        <span className="absolute top-2 left-2 bg-foreground text-background text-xs px-2 py-0.5">
          {image.moderation_reason ?? "rejected"}
        </span>
        {image.is_primary && (
          <span className="absolute top-2 right-2 bg-background text-foreground text-xs px-2 py-0.5 border border-border">
            primary
          </span>
        )}
      </div>
      <div className="p-3">
        <div className="text-sm">
          <Link
            href={`/artists/${encodeURIComponent(image.artist_slug)}`}
            className="hover:underline"
          >
            {image.artist_display_name}
          </Link>{" "}
          <span className="text-muted">·</span>{" "}
          {image.artwork_title ?? <em className="text-muted">untitled</em>}
        </div>
        <div className="mt-3 flex items-center gap-2">
          <button
            type="button"
            disabled={isPending}
            onClick={onOverride}
            className="px-3 py-1.5 text-sm border border-foreground bg-foreground text-background disabled:opacity-40"
          >
            Override (approve)
          </button>
        </div>
      </div>
    </li>
  );
}
