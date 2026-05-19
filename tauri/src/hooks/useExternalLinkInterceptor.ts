import { useEffect } from "react";
import { api } from "../api";

// Capture-phase click listener on document that catches anchor clicks
// before React Router's internal handlers run. If the anchor has an
// absolute http/https URL, prevent the default navigation (which would
// either replace the Tauri webview or pop a useless empty window) and
// forward to the system browser via the Rust open_url command.
//
// React Router <Link>s render anchors with relative hrefs (`/dashboard`,
// `/course/123`), so the absolute-URL gate leaves them alone.

export function useExternalLinkInterceptor() {
  useEffect(() => {
    function onClick(event: MouseEvent) {
      // Modifier-click (cmd/ctrl/shift/alt) usually means the user wants
      // a specific browser behavior — leave it alone. Same for middle
      // mouse / non-primary buttons.
      if (event.button !== 0) return;
      if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;

      // Find the anchor in the event path. `event.target` could be the
      // text or icon inside the link, not the <a> itself.
      const anchor = (event.target as HTMLElement | null)?.closest("a") as HTMLAnchorElement | null;
      if (!anchor) return;

      const href = anchor.getAttribute("href");
      if (!href) return;

      // Only intercept absolute http(s) URLs. Skip in-app routes
      // (`/dashboard`), anchors (`#section`), and Tauri / blob / data URIs.
      if (!/^https?:\/\//i.test(href)) return;

      event.preventDefault();
      event.stopPropagation();
      api.openUrl(href).catch((e) => {
        console.error("openUrl failed", href, e);
      });
    }

    document.addEventListener("click", onClick, true);
    return () => document.removeEventListener("click", onClick, true);
  }, []);
}
