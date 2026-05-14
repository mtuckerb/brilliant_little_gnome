// Settings → Device Sync panel (T-015).
//
// Surfaces the six p2p_* commands wired in T-014. Two modes:
//
//   disabled
//     Single toggle ("Sync between my devices"). On enable: invoke
//     p2p_enable, which generates secrets if needed and starts the
//     engine; flip into the enabled view.
//
//   enabled
//     Truncated NodeId, paired-peer count, two action buttons.
//       1. "Show pairing QR" — invoke p2p_pairing_qr, render the
//          returned PNG inline; start a 60s countdown that hides the
//          QR when it expires (matches the payload's `exp`).
//       2. "Pair this device" — paste the QR's encoded base64 (e.g.
//          from a phone scanner that posts to clipboard) and invoke
//          p2p_consume_pairing. Mobile builds (T-018) wire the
//          actual camera scanner via the Tauri 2 barcode plugin.
//     A "Rotate secret" button (destructive — invalidates all paired
//     devices) lives below, behind a confirm.
//
// Live updates: subscribes to `p2p:peer_connected` /
// `p2p:peer_disconnected` / `p2p:applied` / `p2p:error` and surfaces
// each via the existing ToastProvider. The status row also re-fetches
// after any of these so the peer count stays honest.

import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "../api";
import { useToast } from "./ToastProvider";
import type { P2pStatus, PairingQr, StorageStats } from "../types";

const PAIR_TTL_SECONDS = 60;
const STORAGE_POLL_MS = 30_000;

export default function SyncPanel() {
  const toast = useToast();
  const [status, setStatus] = useState<P2pStatus | null>(null);
  // `null` = "haven't asked yet"; `false` = "asked, p2p commands aren't
  // present in this build" (no-network feature flag). The latter
  // suppresses the whole panel rather than rendering an error.
  const [available, setAvailable] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [qr, setQr] = useState<PairingQr | null>(null);
  const [qrCountdown, setQrCountdown] = useState<number>(0);
  const [pasteOpen, setPasteOpen] = useState(false);
  const [pasteText, setPasteText] = useState("");
  const [storage, setStorage] = useState<StorageStats | null>(null);
  const countdownRef = useRef<number | null>(null);

  const refresh = useCallback(async () => {
    try {
      const s = await api.p2pStatus();
      setStatus(s);
      setAvailable(true);
    } catch (e) {
      // The backend was likely built without `p2p` — `invoke` rejects
      // when the command symbol isn't registered. Hide the panel
      // rather than surfacing a confusing error to the user.
      console.warn("p2p_status unavailable:", e);
      setAvailable(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Subscribe to live events. Each tear-down also clears the
  // countdown interval so the cleanup is total when the panel
  // unmounts.
  useEffect(() => {
    if (available !== true) return;
    const unlisteners: UnlistenFn[] = [];
    (async () => {
      unlisteners.push(
        await listen<{ node_id: string }>("p2p:peer_connected", (e) => {
          toast.show(`Peer connected: ${truncate(e.payload.node_id)}`, "is-success");
          refresh();
        }),
      );
      unlisteners.push(
        await listen<{ node_id: string }>("p2p:peer_disconnected", (e) => {
          toast.show(`Peer disconnected: ${truncate(e.payload.node_id)}`, "is-warning");
          refresh();
        }),
      );
      unlisteners.push(
        await listen<{ kind: string; id: string }>("p2p:applied", (e) => {
          toast.show(`Synced ${e.payload.kind} <code>${e.payload.id}</code>`);
        }),
      );
      unlisteners.push(
        await listen<{ message: string }>("p2p:error", (e) => {
          toast.show(`Sync error: ${e.payload.message}`, "is-danger");
        }),
      );
      unlisteners.push(
        await listen<{ message: string }>("p2p:warning", (e) => {
          toast.show(e.payload.message, "is-warning", 10_000);
        }),
      );
    })();
    return () => {
      for (const u of unlisteners) u();
    };
  }, [available, refresh, toast]);

  // Poll storage stats while the engine is running. 30s feels right
  // — these numbers move on minute scales (one checkpoint per
  // minute), so faster polling would just be UI churn.
  useEffect(() => {
    if (!status?.enabled || !status.nodeId) {
      setStorage(null);
      return;
    }
    let alive = true;
    const tick = async () => {
      try {
        const s = await api.p2pStorageStats();
        if (alive) setStorage(s);
      } catch {
        // Engine isn't running yet — keep showing the previous value
        // (if any) and try again on the next tick.
      }
    };
    tick();
    const id = window.setInterval(tick, STORAGE_POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, [status?.enabled, status?.nodeId]);

  // QR countdown — ticks each second, hides QR when reaching 0. We
  // start the timer when the QR is fetched (so the user-visible
  // expiry tracks the payload's `exp`, not the moment they opened the
  // panel).
  useEffect(() => {
    if (qrCountdown <= 0) {
      if (countdownRef.current !== null) {
        window.clearInterval(countdownRef.current);
        countdownRef.current = null;
      }
      if (qr) setQr(null);
      return;
    }
    countdownRef.current = window.setInterval(() => {
      setQrCountdown((n) => Math.max(0, n - 1));
    }, 1000);
    return () => {
      if (countdownRef.current !== null) {
        window.clearInterval(countdownRef.current);
        countdownRef.current = null;
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [qrCountdown]);

  if (available === false) return null;
  if (available === null || !status) {
    return (
      <div className="box">
        <h2 className="title is-5">Device Sync</h2>
        <p className="has-text-grey">Loading…</p>
      </div>
    );
  }

  async function toggle(enable: boolean) {
    setBusy(true);
    try {
      if (enable) {
        const s = await api.p2pEnable();
        setStatus(s);
        toast.show("Device sync enabled.", "is-success");
      } else {
        await api.p2pDisable();
        await refresh();
        setQr(null);
        setQrCountdown(0);
        toast.show("Device sync disabled.");
      }
    } catch (e) {
      toast.show(`p2p toggle failed: ${e}`, "is-danger");
    } finally {
      setBusy(false);
    }
  }

  async function showQr() {
    setBusy(true);
    try {
      const next = await api.p2pPairingQr();
      setQr(next);
      const remaining = Math.max(
        0,
        Math.min(PAIR_TTL_SECONDS, next.payload.exp - Math.floor(Date.now() / 1000)),
      );
      setQrCountdown(remaining || PAIR_TTL_SECONDS);
    } catch (e) {
      toast.show(`Could not generate QR: ${e}`, "is-danger");
    } finally {
      setBusy(false);
    }
  }

  async function consumePaste() {
    const text = pasteText.trim();
    if (!text) return;
    setBusy(true);
    try {
      const s = await api.p2pConsumePairing(text);
      setStatus(s);
      setPasteText("");
      setPasteOpen(false);
      toast.show("Paired — initial sync starting.", "is-success");
    } catch (e) {
      toast.show(`Pairing failed: ${e}`, "is-danger");
    } finally {
      setBusy(false);
    }
  }

  // Mobile-only camera scanner. Dynamic-imports the barcode plugin so
  // desktop bundles don't fail at load time when the plugin's IPC
  // commands aren't registered. On any error (no camera, denied, or
  // desktop platform) we surface the cause and pop open the paste form
  // as the universal fallback.
  async function scanQr() {
    setBusy(true);
    try {
      const mod = await import("@tauri-apps/plugin-barcode-scanner");
      let perm = await mod.checkPermissions();
      if (perm === "prompt") {
        perm = await mod.requestPermissions();
      }
      if (perm !== "granted") {
        toast.show("Camera permission denied.", "is-warning");
        setPasteOpen(true);
        return;
      }
      const result = await mod.scan({
        windowed: false,
        formats: [mod.Format.QRCode],
      });
      if (!result?.content) return;
      const s = await api.p2pConsumePairing(result.content);
      setStatus(s);
      toast.show("Paired — initial sync starting.", "is-success");
    } catch (e) {
      console.warn("QR scan unavailable, falling back to paste:", e);
      toast.show(`Scanner unavailable: ${e}`, "is-warning");
      setPasteOpen(true);
    } finally {
      setBusy(false);
    }
  }

  async function rotate() {
    if (
      !window.confirm(
        "Rotate the sync secret? All other paired devices will fall off and need to be re-paired with a new QR.",
      )
    ) {
      return;
    }
    setBusy(true);
    try {
      const s = await api.p2pRotate();
      setStatus(s);
      setQr(null);
      setQrCountdown(0);
      toast.show("Sync secret rotated.", "is-warning");
    } catch (e) {
      toast.show(`Rotate failed: ${e}`, "is-danger");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="box">
      <h2 className="title is-5">Device Sync</h2>
      <p className="is-size-7 has-text-grey mb-3">
        Encrypted P2P sync of your overlays (pinned courses, custom colors,
        notes, completion ticks) across your own devices. Brightspace data
        itself stays on each device — only your edits travel.
      </p>

      <div className="field">
        <label className="checkbox">
          <input
            type="checkbox"
            checked={status.enabled}
            onChange={(e) => toggle(e.target.checked)}
            disabled={busy}
          />{" "}
          Sync between my devices
        </label>
      </div>

      {status.enabled && (
        <>
          <div className="field">
            <label className="label is-small">This device</label>
            <p className="is-family-monospace is-size-7">
              {status.nodeId ? truncate(status.nodeId) : <em>(not running)</em>}
            </p>
            <p className="is-size-7 has-text-grey">
              {status.pairedPeers.length} paired peer
              {status.pairedPeers.length === 1 ? "" : "s"}
            </p>
          </div>

          <div className="buttons">
            <button
              className="button is-primary"
              onClick={showQr}
              disabled={busy || !status.nodeId}
            >
              <span className="icon"><i className="fas fa-qrcode" /></span>
              <span>Show pairing QR</span>
            </button>
            <button
              className="button"
              onClick={scanQr}
              disabled={busy}
              title="Mobile: use the camera. Desktop falls back to manual paste."
            >
              <span className="icon"><i className="fas fa-camera" /></span>
              <span>Scan pairing QR</span>
            </button>
            <button
              className="button"
              onClick={() => setPasteOpen((v) => !v)}
              disabled={busy}
            >
              <span className="icon"><i className="fas fa-link" /></span>
              <span>Paste pairing data</span>
            </button>
          </div>

          {qr && qrCountdown > 0 && (
            <div className="notification mt-3">
              <p className="is-size-7 has-text-grey mb-2">
                Scan this on the joining device. Expires in {qrCountdown}s.
              </p>
              <img
                src={`data:image/png;base64,${qr.pngB64}`}
                alt="Pairing QR"
                style={{ width: 256, height: 256, imageRendering: "pixelated" }}
              />
              <details className="mt-2">
                <summary className="is-size-7 has-text-grey">
                  Manual paste
                </summary>
                <textarea
                  className="textarea is-small mt-2 is-family-monospace"
                  rows={4}
                  readOnly
                  value={qr.encoded}
                  onFocus={(e) => e.currentTarget.select()}
                />
              </details>
            </div>
          )}

          {pasteOpen && (
            <div className="notification mt-3">
              <p className="is-size-7 has-text-grey mb-2">
                Paste the QR data from the seed device. (Mobile builds use the
                camera scanner; desktop falls back to manual paste.)
              </p>
              <textarea
                className="textarea is-small is-family-monospace"
                rows={4}
                value={pasteText}
                onChange={(e) => setPasteText(e.target.value)}
                placeholder="eyJ2IjoxLCJub2RlIjoi…"
              />
              <div className="buttons mt-2">
                <button
                  className="button is-primary is-small"
                  onClick={consumePaste}
                  disabled={busy || !pasteText.trim()}
                >
                  Pair
                </button>
                <button
                  className="button is-small"
                  onClick={() => {
                    setPasteOpen(false);
                    setPasteText("");
                  }}
                  disabled={busy}
                >
                  Cancel
                </button>
              </div>
            </div>
          )}

          {storage && (
            <div className="mt-3 has-text-grey is-size-7">
              Storage: snapshot {fmtBytes(storage.snapshotBytes)} · WAL{" "}
              {fmtBytes(storage.walBytes)} ({storage.walEntries} entries)
            </div>
          )}

          <hr />
          <div>
            <button
              className="button is-warning is-small"
              onClick={rotate}
              disabled={busy}
            >
              <span className="icon"><i className="fas fa-rotate" /></span>
              <span>Rotate secret (forget all paired devices)</span>
            </button>
          </div>
        </>
      )}
    </div>
  );
}

/// Show only the head + tail of long iroh node IDs so the UI doesn't
/// wrap awkwardly. The truncation is purely cosmetic — the full ID
/// is always available in the QR payload.
function truncate(s: string, head = 8, tail = 6): string {
  if (s.length <= head + tail + 1) return s;
  return `${s.slice(0, head)}…${s.slice(-tail)}`;
}

/// Render a byte count as KB/MB depending on magnitude. We use SI
/// (1000-based) units rather than binary because that's what the
/// backend reports (`fs::metadata` len in bytes) and the user-facing
/// "MB" most people compare against in tools is decimal too.
function fmtBytes(n: number): string {
  if (n < 1_000) return `${n} B`;
  if (n < 1_000_000) return `${(n / 1_000).toFixed(1)} KB`;
  return `${(n / 1_000_000).toFixed(1)} MB`;
}
