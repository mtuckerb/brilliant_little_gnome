import { useEffect, useState } from "react";
import { api } from "../api";
import type { SyncStatus } from "../types";

// Gnome-shaped sync indicator. The whole gnome fills with one color; the
// liquid level + surface wave reflect activity:
//
//   idle           → fill = 100%, no slosh (surface flat)
//   auth degraded  → fill drains to ~35% (problem cue, dramatic)
//   syncing        → fill height tracks progress (0–100), surface sloshes
//                    side-to-side via a translating SVG wave
//   p2p apply      → quick "blip" slosh on top of the rest state
//
// Visually one mark, one color. Two-tone split removed per Tucker's note
// — the gnome reads as a gnome at a glance.

interface Props {
  size?: number;
  brightspaceSync?: SyncStatus | null;
  brightspaceOk?: boolean;
}

interface P2pState { enabled: boolean; peers: number; lastApplyAt: string | null }

export default function GnomeSync({ size = 24, brightspaceSync, brightspaceOk = true }: Props) {
  const [p2p, setP2p] = useState<P2pState>({ enabled: false, peers: 0, lastApplyAt: null });

  useEffect(() => {
    let alive = true;
    async function pull() {
      try {
        const s = await api.p2pStatus();
        if (!alive) return;
        setP2p({ enabled: s.enabled, peers: s.pairedPeers?.length ?? 0, lastApplyAt: s.lastApplyAt });
      } catch {
        if (alive) setP2p({ enabled: false, peers: 0, lastApplyAt: null });
      }
    }
    pull();
    const t = setInterval(pull, 5000);
    return () => { alive = false; clearInterval(t); };
  }, []);

  const brightspaceActive = brightspaceSync?.status === "syncing";
  const recentApply = (() => {
    if (!p2p.lastApplyAt) return false;
    const ts = Date.parse(p2p.lastApplyAt);
    return Number.isFinite(ts) && Date.now() - ts < 5_000;
  })();
  const active = brightspaceActive || recentApply;

  // Fill height: during a sync, follow the progress so the water visibly
  // rises as syncing nears completion. P2P apply is a flash event — use
  // a temporary partial level to make the surface visibly drop and rise.
  let fillPct: number;
  if (!brightspaceOk) {
    fillPct = 35;
  } else if (brightspaceActive) {
    const p = brightspaceSync?.progress ?? 0;
    // Keep it visibly partial: 20% floor, climb up to 95% as progress runs;
    // pinning at 100 prematurely would hide the slosh.
    fillPct = Math.max(20, Math.min(95, Math.round(p * 0.95)));
  } else if (recentApply) {
    fillPct = 75;
  } else {
    fillPct = 100;
  }

  const title = [
    brightspaceActive
      ? `Brightspace: ${brightspaceSync?.current_task ?? "syncing"} ${brightspaceSync?.progress ?? 0}%`
      : `Brightspace: ${brightspaceOk ? "synced" : "session expired"}`,
    p2p.enabled ? `P2P: ${p2p.peers} peer${p2p.peers === 1 ? "" : "s"}${recentApply ? " · applied" : ""}` : "P2P: off",
  ].join("  ·  ");

  return (
    <div
      title={title}
      aria-label={title}
      style={{ width: size, height: size, position: "relative", display: "inline-block" }}
    >
      {/* Body: colored fill clipped by the silhouette mask. */}
      <div
        style={{
          position: "absolute",
          inset: 0,
          overflow: "hidden",
          WebkitMaskImage: "url('/logo-silhouette.png')",
          WebkitMaskRepeat: "no-repeat",
          WebkitMaskSize: "contain",
          WebkitMaskPosition: "center",
          maskImage: "url('/logo-silhouette.png')",
          maskRepeat: "no-repeat",
          maskSize: "contain",
          maskPosition: "center",
          background: "rgba(115, 154, 195, 0.12)",
        }}
      >
        <div
          style={{
            position: "absolute",
            left: 0,
            right: 0,
            bottom: 0,
            height: `${fillPct}%`,
            background: "#739AC3",
            transition: "height 0.8s ease",
          }}
        >
          {/* Surface wave — only visible when active. The SVG is 2x wide so
              translating by -50% loops one full wavelength seamlessly. */}
          {active && (
            <svg
              viewBox="0 0 200 20"
              preserveAspectRatio="none"
              style={{
                position: "absolute",
                top: -8,
                left: 0,
                width: "200%",
                height: 14,
                animation: "gnome-wave 2.4s linear infinite",
                display: "block",
              }}
              aria-hidden
            >
              <path
                d="M0 12 Q 12.5 4, 25 12 T 50 12 T 75 12 T 100 12 T 125 12 T 150 12 T 175 12 T 200 12 L 200 20 L 0 20 Z"
                fill="#739AC3"
              />
            </svg>
          )}
        </div>
      </div>
      {/* Face features stay opaque on top so the gnome reads as a gnome. */}
      <img
        src="/logo-features.png"
        alt=""
        aria-hidden
        style={{
          position: "absolute",
          inset: 0,
          width: "100%",
          height: "100%",
          objectFit: "contain",
          pointerEvents: "none",
        }}
      />
      <style>{`
        @keyframes gnome-wave {
          0%   { transform: translateX(0); }
          100% { transform: translateX(-50%); }
        }
      `}</style>
    </div>
  );
}
