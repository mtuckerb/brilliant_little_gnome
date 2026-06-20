import { useEffect, useState } from "react";

// Decide whether to render MobileLayout (bottom-nav, no navrail) vs
// DesktopVision (left navrail). Two independent triggers, OR'd together:
//
//   1. Real touch device → ALWAYS mobile. A phone in landscape can be wider
//      than the phone breakpoint (e.g. ~844–932px), and the desktop navrail
//      must never appear on a phone in any orientation. We detect this with a
//      coarse primary pointer + no true hover capability, which is true for
//      phones/tablets but false for a desktop (fine pointer) and for
//      touchscreen laptops (which also report hover via their trackpad).
//   2. Narrow window → mobile. Preserves the iPad-on-the-side workflow where
//      a desktop window is dragged narrow to preview the mobile shell live.
//
// 640px is the standard "phone" width cutoff for trigger #2.

const MOBILE_MAX_PX = 640;

function isTouchPhone(): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return false;
  }
  return (
    window.matchMedia("(pointer: coarse)").matches &&
    !window.matchMedia("(any-hover: hover)").matches
  );
}

function computeIsMobile(): boolean {
  if (typeof window === "undefined") return false;
  return isTouchPhone() || window.innerWidth <= MOBILE_MAX_PX;
}

export function useIsMobile(): boolean {
  const [isMobile, setIsMobile] = useState(computeIsMobile);
  useEffect(() => {
    function onChange() {
      setIsMobile(computeIsMobile());
    }
    window.addEventListener("resize", onChange);
    // Orientation/pointer changes don't always fire `resize`; subscribe to the
    // pointer media query too so a phone rotating to landscape stays mobile.
    const mq =
      typeof window.matchMedia === "function"
        ? window.matchMedia("(pointer: coarse)")
        : null;
    mq?.addEventListener?.("change", onChange);
    return () => {
      window.removeEventListener("resize", onChange);
      mq?.removeEventListener?.("change", onChange);
    };
  }, []);
  return isMobile;
}
