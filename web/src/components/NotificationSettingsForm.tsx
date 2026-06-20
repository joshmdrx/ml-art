"use client";

/**
 * T-068 — toggles for each notification kind plus a master kill switch.
 *
 * Optimistic-update semantics: a click flips the local state immediately,
 * then a server action persists it. If the server action errors, we revert
 * and show the message.
 */

import { useState, useTransition } from "react";
import type { NotificationPreferences } from "@/lib/api";
import { toUserMessage } from "@/lib/reportError";
import { setNotificationPreferences } from "@/app/actions/notifications";

interface KindMeta {
  label: string;
  description: string;
}

export function NotificationSettingsForm({
  initial,
  kindMeta,
}: {
  initial: NotificationPreferences;
  kindMeta: Record<string, KindMeta>;
}) {
  const [prefs, setPrefs] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const [pending, startTransition] = useTransition();

  function applyOptimistic(
    next: NotificationPreferences,
    patch: { global_enabled?: boolean; kinds?: Record<string, boolean> },
  ) {
    const previous = prefs;
    setPrefs(next);
    setError(null);
    startTransition(async () => {
      try {
        const updated = await setNotificationPreferences(patch);
        setPrefs(updated);
      } catch (e) {
        setPrefs(previous);
        setError(
          toUserMessage(e, "Couldn't save that. Try again.", {
            surface: "notification-settings-form",
          }),
        );
      }
    });
  }

  function toggleKind(kind: string) {
    const nextValue = !prefs.kinds[kind];
    applyOptimistic(
      { ...prefs, kinds: { ...prefs.kinds, [kind]: nextValue } },
      { kinds: { [kind]: nextValue } },
    );
  }

  function toggleGlobal() {
    const nextValue = !prefs.global_enabled;
    applyOptimistic({ ...prefs, global_enabled: nextValue }, {
      global_enabled: nextValue,
    });
  }

  // Stable display order — sort by label for now (only one kind today;
  // when more land, may want to group by audience or type).
  const sortedKinds = Object.keys(prefs.kinds).sort((a, b) => {
    const la = kindMeta[a]?.label ?? a;
    const lb = kindMeta[b]?.label ?? b;
    return la.localeCompare(lb);
  });

  return (
    <div className="space-y-8">
      <section className="border border-border bg-surface p-5">
        <Row
          label="All notification emails"
          description={
            prefs.global_enabled
              ? "Notification emails are on. Per-kind toggles below decide which ones you actually receive."
              : "All notification emails are off. Transactional emails (inquiry verification, replies to inquiries you sent) still go through."
          }
          checked={prefs.global_enabled}
          onChange={toggleGlobal}
          disabled={pending}
        />
      </section>

      <section
        className={
          "border border-border " +
          (prefs.global_enabled ? "bg-surface" : "bg-background opacity-60")
        }
      >
        <ul className="divide-y divide-border">
          {sortedKinds.map((kind) => {
            const meta = kindMeta[kind] ?? {
              label: kind,
              description: "",
            };
            return (
              <li key={kind} className="p-5">
                <Row
                  label={meta.label}
                  description={meta.description}
                  checked={prefs.kinds[kind] === true}
                  onChange={() => toggleKind(kind)}
                  disabled={pending || !prefs.global_enabled}
                />
              </li>
            );
          })}
        </ul>
      </section>

      {error && (
        <p className="text-sm text-foreground border border-border bg-surface p-3">
          {error}
        </p>
      )}
    </div>
  );
}

function Row({
  label,
  description,
  checked,
  onChange,
  disabled,
}: {
  label: string;
  description: string;
  checked: boolean;
  onChange: () => void;
  disabled: boolean;
}) {
  return (
    <div className="flex items-start justify-between gap-6">
      <div className="flex-1">
        <p className="font-medium">{label}</p>
        {description && (
          <p className="mt-1 text-sm text-muted leading-relaxed">
            {description}
          </p>
        )}
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={onChange}
        disabled={disabled}
        className={
          "shrink-0 mt-1 relative inline-flex h-6 w-11 items-center transition-colors disabled:opacity-50 " +
          (checked ? "bg-foreground" : "bg-border")
        }
      >
        <span
          aria-hidden
          className={
            "inline-block h-5 w-5 bg-background transition-transform " +
            (checked ? "translate-x-5" : "translate-x-0.5")
          }
        />
      </button>
    </div>
  );
}
