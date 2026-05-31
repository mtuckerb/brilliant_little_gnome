import { useEffect, useState } from "react";

// Track whether the viewport is in the mobile range. 640px is the standard
// "phone" cutoff — below it we render MobileLayout, above we render
// DesktopVision. The hook subscribes to resize so the shell swaps live
// during window dragging on a desktop (helpful during the iPad-on-the-side
// kind of workflow Tucker does).

const MOBILE_MAX_PX = 640;

export function useIsMobile(): boolean {
  const [isMobile, setIsMobile] = useState(() =>
    typeof window !== "undefined" ? window.innerWidth <= MOBILE_MAX_PX : false,
  );
  useEffect(() => {
    function onResize() {
      setIsMobile(window.innerWidth <= MOBILE_MAX_PX);
    }
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  return isMobile;
}
