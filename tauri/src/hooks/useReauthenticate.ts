import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "../api";
import type { AuthStatus } from "../types";

interface ReauthenticateState {
  busy: boolean;
  error: string | null;
  reauthenticate: () => Promise<void>;
}

export function useReauthenticate(
  host: string | null,
  onComplete: (auth: AuthStatus) => void,
): ReauthenticateState {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const onCompleteRef = useRef(onComplete);

  useEffect(() => {
    onCompleteRef.current = onComplete;
  }, [onComplete]);

  useEffect(() => {
    let unlistenCaptured: UnlistenFn | null = null;
    let unlistenTimeout: UnlistenFn | null = null;
    let cancelled = false;

    listen<string>("auth-captured", async () => {
      try {
        const auth = await api.authStatus();
        if (!cancelled) {
          onCompleteRef.current(auth);
          setError(null);
        }
      } catch (e: any) {
        if (!cancelled) setError(String(e?.message ?? e));
      } finally {
        if (!cancelled) setBusy(false);
      }
    }).then((u) => {
      if (cancelled) u();
      else unlistenCaptured = u;
    });

    listen<string>("auth-capture-timeout", () => {
      if (!cancelled) {
        setError("Login timed out. Please try again.");
        setBusy(false);
      }
    }).then((u) => {
      if (cancelled) u();
      else unlistenTimeout = u;
    });

    return () => {
      cancelled = true;
      unlistenCaptured?.();
      unlistenTimeout?.();
    };
  }, []);

  const reauthenticate = useCallback(async () => {
    if (!host) {
      setError("No Brightspace host is configured.");
      return;
    }

    setBusy(true);
    setError(null);
    try {
      await api.openLoginWindow(host);
    } catch (e: any) {
      setError(String(e?.message ?? e));
      setBusy(false);
    }
  }, [host]);

  return { busy, error, reauthenticate };
}
