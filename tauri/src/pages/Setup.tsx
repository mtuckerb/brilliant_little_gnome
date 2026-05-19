import { useState } from "react";
import { api } from "../api";
import { useReauthenticate } from "../hooks/useReauthenticate";
import type { AuthStatus } from "../types";
import { useIsMobile } from "../hooks/useIsMobile";

interface Props {
  onComplete: (auth: AuthStatus) => void;
}

export default function Setup({ onComplete }: Props) {
  const [host, setHost] = useState("courses.maine.edu");
  const [cookies, setCookies] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const browserLogin = useReauthenticate(host, onComplete);
  const isMobile = useIsMobile();

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setErr(null);
    try {
      const auth = await api.setupCookies(host, cookies);
      onComplete(auth);
    } catch (e: any) {
      setErr(String(e?.message ?? e));
    } finally {
      setBusy(false);
    }
  }

  // Native cookie capture: opens a real Brightspace login in a Tauri webview
  // window. The backend polls the runtime cookie store; when both
  // d2lSessionVal + d2lSecureSessionVal arrive it persists them and emits
  // `auth-captured`.
  async function loginWithBrowser() {
    setErr(null);
    await browserLogin.reauthenticate();
  }

  return (
    <div className="main-content container" style={{ maxWidth: 640 }}>
      <h1 className="title">Welcome to Brilliant</h1>
      <p className="subtitle">Connect to your Brightspace account.</p>
      {isMobile && (
        <div className="notification is-info is-light">
          <p className="mb-2"><strong>On mobile?</strong> The easiest path is to sign in once on a desktop or laptop and then pair this phone with it.</p>
          <ol style={{ paddingLeft: 18, margin: 0 }} className="is-size-7">
            <li>Sign in on a paired Mac/PC.</li>
            <li>In Settings → Device pairing on the desktop, generate a QR code.</li>
            <li>Scan it here — your session syncs automatically.</li>
          </ol>
          <p className="is-size-7 mt-2 has-text-grey">You can also paste cookies below if you've grabbed them from a browser, but native in-app sign-in for mobile is still on the punch list.</p>
        </div>
      )}
      <form onSubmit={submit} className="box">
        <div className="field">
          <label className="label">Brightspace host</label>
          <div className="control">
            <input className="input" value={host} onChange={(e) => setHost(e.target.value)} placeholder="courses.maine.edu" />
          </div>
        </div>
        <div className="field">
          <label className="label">Session cookies (paste from your browser, optional)</label>
          <div className="control">
            <textarea className="textarea" rows={4} value={cookies} onChange={(e) => setCookies(e.target.value)} placeholder="d2lSessionVal=...; d2lSecureSessionVal=..." />
          </div>
          <p className="help">Either paste your cookies directly, or use the login button below to capture them in a Brightspace browser window.</p>
        </div>
        {(err || browserLogin.error) && <div className="notification is-danger is-light">{err || browserLogin.error}</div>}
        <div className="field is-grouped">
          <div className="control">
            <button type="submit" className={`button is-primary ${busy ? "is-loading" : ""}`} disabled={busy || !host}>Save</button>
          </div>
          {!isMobile && (
            <div className="control">
              <button type="button" className={`button is-link is-light ${browserLogin.busy ? "is-loading" : ""}`} onClick={loginWithBrowser} disabled={busy || browserLogin.busy || !host}>
                Log in with browser
              </button>
            </div>
          )}
        </div>
      </form>
    </div>
  );
}
