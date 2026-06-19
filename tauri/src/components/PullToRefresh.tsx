import { useEffect, useRef, useState, type ReactNode } from "react";

// Pull-to-refresh for the mobile shell. iOS WKWebView only gives native
// pull-to-refresh to the top-level document scroll, but our content scrolls in
// an inner pane — so we implement the gesture by hand on that pane: when the
// user is at the top and drags down past a threshold, we fire `onRefresh` and
// hold a spinner until it resolves. Inert on desktop (no touch events fire).

interface Props {
  /** Runs on a past-threshold pull. Resolve it when the refresh truly
   *  finishes — the spinner is held for the whole promise. */
  onRefresh: () => Promise<void>;
  children: ReactNode;
  className?: string;
  style?: React.CSSProperties;
}

const THRESHOLD = 64; // px of pull needed to arm a refresh
const MAX_PULL = 96; // clamp so the content can't be dragged arbitrarily far
const SPINNER_HEIGHT = 48; // resting height of the indicator while refreshing
const RESISTANCE = 0.5; // finger travel -> content travel (rubber-band feel)

export default function PullToRefresh({ onRefresh, children, className, style }: Props) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [pull, setPull] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const [dragging, setDragging] = useState(false);

  // Refs mirror state for the native (closure-captured) touch listeners.
  const pullRef = useRef(0);
  const pullingRef = useRef(false);
  const startYRef = useRef(0);
  const refreshingRef = useRef(false);

  const setPullBoth = (v: number) => {
    pullRef.current = v;
    setPull(v);
  };

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;

    const onStart = (e: TouchEvent) => {
      if (refreshingRef.current || el.scrollTop > 0) {
        pullingRef.current = false;
        return;
      }
      startYRef.current = e.touches[0].clientY;
      pullingRef.current = true;
    };

    const onMove = (e: TouchEvent) => {
      if (!pullingRef.current || refreshingRef.current) return;
      const dy = e.touches[0].clientY - startYRef.current;
      if (dy <= 0) {
        // Scrolling up / no downward pull — yield to normal scrolling.
        if (pullRef.current !== 0) setPullBoth(0);
        return;
      }
      if (el.scrollTop > 0) {
        pullingRef.current = false;
        setDragging(false);
        if (pullRef.current !== 0) setPullBoth(0);
        return;
      }
      if (!dragging) setDragging(true);
      setPullBoth(Math.min(dy * RESISTANCE, MAX_PULL));
      // Suppress the native rubber-band overscroll while we own the gesture.
      e.preventDefault();
    };

    const onEnd = () => {
      if (!pullingRef.current) return;
      pullingRef.current = false;
      setDragging(false);
      if (pullRef.current > THRESHOLD && !refreshingRef.current) {
        refreshingRef.current = true;
        setRefreshing(true);
        setPullBoth(SPINNER_HEIGHT);
        Promise.resolve(onRefresh())
          .catch(() => {})
          .finally(() => {
            refreshingRef.current = false;
            setRefreshing(false);
            setPullBoth(0);
          });
      } else {
        setPullBoth(0);
      }
    };

    // `passive: false` so preventDefault() actually suppresses the iOS bounce.
    el.addEventListener("touchstart", onStart, { passive: true });
    el.addEventListener("touchmove", onMove, { passive: false });
    el.addEventListener("touchend", onEnd, { passive: true });
    el.addEventListener("touchcancel", onEnd, { passive: true });
    return () => {
      el.removeEventListener("touchstart", onStart);
      el.removeEventListener("touchmove", onMove);
      el.removeEventListener("touchend", onEnd);
      el.removeEventListener("touchcancel", onEnd);
    };
    // onRefresh is captured fresh each render via the ref-free closure above;
    // re-attaching on every change is unnecessary and would drop in-flight drags.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const indicatorH = refreshing ? SPINNER_HEIGHT : pull;
  const armed = !refreshing && pull > THRESHOLD;

  return (
    <div
      ref={scrollRef}
      className={className}
      style={{
        position: "relative",
        overflowY: "auto",
        overscrollBehaviorY: "contain",
        WebkitOverflowScrolling: "touch",
        minHeight: 0,
        ...style,
      }}
    >
      <div
        aria-hidden
        style={{
          position: "absolute",
          top: 0,
          left: 0,
          right: 0,
          height: indicatorH,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: "var(--pencil-text-secondary)",
          pointerEvents: "none",
          opacity: refreshing ? 1 : Math.min(pull / THRESHOLD, 1),
          transition: dragging ? "none" : "height .2s ease, opacity .2s ease",
        }}
      >
        <i
          className={`fas ${refreshing ? "fa-circle-notch fa-spin" : "fa-arrow-down"}`}
          style={{
            fontSize: 16,
            transform: armed ? "rotate(180deg)" : "none",
            transition: "transform .15s ease",
          }}
        />
      </div>
      <div
        style={{
          transform: `translateY(${indicatorH}px)`,
          transition: dragging ? "none" : "transform .2s ease",
        }}
      >
        {children}
      </div>
    </div>
  );
}
