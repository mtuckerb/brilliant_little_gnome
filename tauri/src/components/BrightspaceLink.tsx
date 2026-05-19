import { useEffect, useState } from "react";
import { api } from "../api";

// Small "open in Brightspace" icon button. Click → opens the URL in the
// user's default browser via the open_url Rust command (skipping
// tauri-plugin-shell's allowlist dance). Designed to be unobtrusive — fits
// next to a title or in an action row without stealing visual weight.

interface Props {
  url: string;
  label?: string;
  /// Icon-only by default; pass `withLabel` when the surrounding context
  /// would otherwise be ambiguous (e.g. top of a page).
  withLabel?: boolean;
  className?: string;
  iconClassName?: string;
}

let cachedHost: string | null | undefined;

export function useBrightspaceHost(): string | null {
  const [host, setHost] = useState<string | null>(cachedHost ?? null);
  useEffect(() => {
    if (cachedHost !== undefined) return;
    api
      .getPrefs()
      .then((p) => {
        cachedHost = p.brightspace_host ?? null;
        setHost(cachedHost);
      })
      .catch(() => {
        cachedHost = null;
      });
  }, []);
  return host;
}

export default function BrightspaceLink({
  url,
  label = "Open in Brightspace",
  withLabel = false,
  className,
  iconClassName,
}: Props) {
  async function onClick(e: React.MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    try {
      await api.openUrl(url);
    } catch (err) {
      alert(`Could not open Brightspace: ${String((err as { message?: string })?.message ?? err)}`);
    }
  }

  return (
    <button
      type="button"
      className={`button is-small is-white ${className ?? ""}`}
      title={label}
      aria-label={label}
      onClick={onClick}
      style={{ background: "transparent", border: "none", padding: "0 6px" }}
    >
      <span className="icon is-small">
        {/* fa-up-right-from-square is the modern "external link" icon —
           small, recognizable, unobtrusive. Matches "rising sun" intent
           by suggesting motion outward / upward to a new context. */}
        <i className={`fas fa-up-right-from-square ${iconClassName ?? "has-text-grey"}`}></i>
      </span>
      {withLabel && <span className="is-size-7 has-text-grey ml-1">{label}</span>}
    </button>
  );
}
