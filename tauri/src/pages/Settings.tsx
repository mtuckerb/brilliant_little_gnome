import { useEffect, useState } from "react";
import { api } from "../api";
import type { UserPreferences } from "../types";

export default function Settings() {
  const [prefs, setPrefs] = useState<UserPreferences | null>(null);
  const [restRunning, setRestRunning] = useState(false);
  const [restPort, setRestPort] = useState<number | null>(null);

  useEffect(() => {
    api.getPrefs().then(setPrefs);
    api.restApiStatus().then((s) => { setRestRunning(s.running); setRestPort(s.port); });
  }, []);

  if (!prefs) return <div className="has-text-centered py-6"><span className="icon is-large has-text-primary"><i className="fas fa-circle-notch fa-spin fa-3x"></i></span></div>;

  function update<K extends keyof UserPreferences>(key: K, value: UserPreferences[K]) {
    api.updatePrefs({ [key]: value } as Partial<UserPreferences>).then(setPrefs);
  }

  async function toggleRest() {
    if (restRunning) {
      await api.restApiStop();
      setRestRunning(false);
      setRestPort(null);
    } else {
      const r = await api.restApiStart();
      setRestRunning(true);
      setRestPort(r.port);
    }
  }

  return (
    <div>
      <h1 className="title"><i className="fas fa-cog mr-2"></i>Settings</h1>
      <div className="box">
        <h2 className="title is-5">Profile</h2>
        <div className="field">
          <label className="label">Display name</label>
          <input className="input" value={prefs.display_name ?? ""} onChange={(e) => update("display_name", e.target.value)} />
        </div>
        <div className="field">
          <label className="label">Time zone (IANA)</label>
          <input className="input" value={prefs.time_zone ?? ""} onChange={(e) => update("time_zone", e.target.value)} placeholder="America/New_York" />
        </div>
      </div>

      <div className="box">
        <h2 className="title is-5">REST API</h2>
        <p className="is-size-7 has-text-grey mb-3">
          Optional embedded HTTP server (axum) for external integrations. Off by default. Authenticated with a static bearer token.
        </p>
        <div className="field">
          <label className="checkbox">
            <input type="checkbox" checked={prefs.api_enabled} onChange={(e) => update("api_enabled", e.target.checked)} /> Enabled
          </label>
        </div>
        <div className="field">
          <label className="label">Listen port</label>
          <input className="input" type="number" value={prefs.api_port} onChange={(e) => update("api_port", parseInt(e.target.value, 10) || 4567)} />
        </div>
        <div className="field">
          <label className="checkbox">
            <input type="checkbox" checked={prefs.api_listen_all} onChange={(e) => update("api_listen_all", e.target.checked)} /> Listen on all interfaces (0.0.0.0)
          </label>
          <p className="help">Off = loopback only (127.0.0.1).</p>
        </div>
        <div className="field">
          <label className="label">Bearer token</label>
          <div className="field has-addons">
            <div className="control is-expanded">
              <input className="input" value={prefs.api_key ?? ""} readOnly />
            </div>
            <div className="control">
              <button className="button" onClick={() => update("api_key", crypto.randomUUID().replace(/-/g, ""))}>
                Regenerate
              </button>
            </div>
          </div>
        </div>
        <div className="field">
          <button className={`button ${restRunning ? "is-danger" : "is-primary"}`} onClick={toggleRest} disabled={!prefs.api_enabled}>
            {restRunning ? "Stop server" : "Start server"}
          </button>
          {restRunning && restPort && (
            <span className="ml-3 has-text-success">Running on port {restPort}</span>
          )}
        </div>
      </div>
    </div>
  );
}
