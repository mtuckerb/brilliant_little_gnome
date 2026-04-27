import { useState } from "react";
import { api } from "../api";
import type { AuthStatus } from "../types";

interface Props {
  onComplete: (auth: AuthStatus) => void;
}

export default function Setup({ onComplete }: Props) {
  const [host, setHost] = useState("courses.maine.edu");
  const [cookies, setCookies] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

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
  // window. Backend listens for navigation and pulls cookies from the session.
  async function loginWithBrowser() {
    setBusy(true);
    setErr(null);
    try {
      const auth = await api.setupCookies(host, "");
      onComplete(auth);
    } catch (e: any) {
      setErr(String(e?.message ?? e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="main-content container" style={{ maxWidth: 640 }}>
      <h1 className="title">Welcome to Brilliant</h1>
      <p className="subtitle">Connect to your Brightspace account.</p>
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
        {err && <div className="notification is-danger is-light">{err}</div>}
        <div className="field is-grouped">
          <div className="control">
            <button type="submit" className={`button is-primary ${busy ? "is-loading" : ""}`} disabled={busy || !host}>Save</button>
          </div>
          <div className="control">
            <button type="button" className="button is-link is-light" onClick={loginWithBrowser} disabled={busy || !host}>
              Log in with browser
            </button>
          </div>
        </div>
      </form>
    </div>
  );
}
