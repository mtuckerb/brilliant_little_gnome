import { useEffect, useState } from "react";
import { api } from "../api";

// A launch failure used to be invisible: Tauri aborts the process when the
// Rust setup hook returns an Err, and on iOS the crash report Apple collects
// carries the stack but not the message. The backend now records those instead
// of aborting, and this surfaces them — the current launch's failure, or the
// panic that killed the previous one.
//
// Deliberately unconditional and above everything else: when this shows, the
// rest of the UI is probably failing its own calls, and this is the only thing
// that explains why.

export default function StartupErrorBanner() {
  const [setupError, setSetupError] = useState<string | null>(null);
  const [previousPanic, setPreviousPanic] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    // An older backend has no such command; failing quietly is right here.
    api.startupError()
      .then((e) => {
        setSetupError(e.setup);
        setPreviousPanic(e.previous_panic);
      })
      .catch(() => {});
  }, []);

  const detail = setupError ?? previousPanic;
  if (!detail || dismissed) return null;

  return (
    <div
      className="notification is-danger is-light mb-0"
      style={{
        borderRadius: 0,
        // This renders above the app's own chrome, so it owns the notch gap;
        // without the inset the first line hides under the status bar.
        paddingTop: "max(env(safe-area-inset-top), 1.25rem)",
      }}
    >
      <button className="delete" aria-label="dismiss" onClick={() => setDismissed(true)} />
      <p className="has-text-weight-semibold">
        {setupError
          ? "Brilliant started without its backend."
          : "Brilliant recovered from a crash on the previous launch."}
      </p>
      <p className="is-size-7 mt-1">
        {setupError
          ? "Data and sync will not work until this is fixed. The error was:"
          : "The last launch stopped before the app opened. The error was:"}
      </p>
      <pre
        className="is-size-7 mt-2"
        style={{ whiteSpace: "pre-wrap", background: "rgba(0,0,0,0.05)", padding: "0.5rem" }}
      >
        {detail}
      </pre>
    </div>
  );
}
