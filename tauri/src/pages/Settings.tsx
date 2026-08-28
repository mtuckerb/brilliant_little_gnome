import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { getName, getVersion, getTauriVersion } from "@tauri-apps/api/app";
import { api, type UpdateInfo } from "../api";
import SyncPanel from "../components/SyncPanel";
import type { UserPreferences } from "../types";
import { devServerHost } from "../buildInfo";

interface BuildInfo {
  appName: string;
  appVersion: string;
  tauriVersion: string;
  devHost: string | null;
}

export default function Settings() {
  const [prefs, setPrefs] = useState<UserPreferences | null>(null);
  const [restRunning, setRestRunning] = useState(false);
  const [restPort, setRestPort] = useState<number | null>(null);
  const [build, setBuild] = useState<BuildInfo | null>(null);

  useEffect(() => {
    api.getPrefs().then(setPrefs);
    api.restApiStatus().then((s) => { setRestRunning(s.running); setRestPort(s.port); });
    Promise.all([getName(), getVersion(), getTauriVersion()])
      .then(([appName, appVersion, tauriVersion]) =>
        setBuild({ appName, appVersion, tauriVersion, devHost: devServerHost() }),
      )
      .catch(() => setBuild(null));
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
        <h2 className="title is-5">Brightspace account</h2>
        <p className="is-size-7 has-text-grey mb-3">
          Session expired or switched accounts? Re-authenticate to refresh your
          Brightspace login (paste cookies, or re-pair with a signed-in desktop).
        </p>
        <Link to="/reauth" className="button is-primary is-light">
          <span className="icon"><i className="fas fa-right-to-bracket"></i></span>
          <span>Re-authenticate</span>
        </Link>
      </div>
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
        <div className="field">
          <label className="label">Default semester</label>
          <input className="input" value={prefs.default_semester ?? ""} onChange={(e) => update("default_semester", e.target.value)} placeholder="2026 Spring" />
          <p className="help">Used as the active filter when one is needed and none is selected.</p>
        </div>
      </div>

      <div className="box">
        <h2 className="title is-5">Cumulative GPA</h2>
        <p className="is-size-7 has-text-grey mb-3">
          Optional pre-existing record so the app can roll new course grades into a running cumulative GPA.
        </p>
        <div className="columns">
          <div className="column">
            <label className="label is-small">Historic GPA</label>
            <input
              className="input"
              type="number"
              step={0.01}
              min={0}
              max={4}
              defaultValue={prefs.historic_gpa ?? ""}
              onBlur={(e) => {
                const v = e.target.value.trim();
                update("historic_gpa", v === "" ? null : parseFloat(v));
              }}
              placeholder="3.50"
            />
          </div>
          <div className="column">
            <label className="label is-small">Historic units</label>
            <input
              className="input"
              type="number"
              step={0.5}
              min={0}
              defaultValue={prefs.historic_units ?? ""}
              onBlur={(e) => {
                const v = e.target.value.trim();
                update("historic_units", v === "" ? null : parseFloat(v));
              }}
              placeholder="60"
            />
          </div>
        </div>
      </div>

      <SyncPanel />

      <div className="box">
        <h2 className="title is-5">Zotero</h2>
        <p className="is-size-7 has-text-grey mb-3">
          Send course content, modules, or individual files to your Zotero library.
          Defaults to your <strong>local Zotero desktop app</strong> — keep Zotero running and enable
          {" "}<em>Preferences → Advanced → "Allow other applications on this computer to communicate with Zotero"</em>.
        </p>
        <div className="field">
          <label className="checkbox">
            <input
              type="checkbox"
              checked={prefs.zotero_use_local}
              onChange={(e) => update("zotero_use_local", e.target.checked)}
            />{" "}
            Use local Zotero (recommended)
          </label>
          <p className="help">Sends to http://127.0.0.1:23119/api. No API key needed.</p>
        </div>
        {prefs.zotero_use_local && (
          <>
            <div className="field">
              <label className="label is-small">Server URL (optional)</label>
              <input
                className="input"
                value={prefs.zotero_local_base_url ?? ""}
                onChange={(e) => update("zotero_local_base_url", e.target.value)}
                placeholder="http://127.0.0.1:23119/api"
              />
              <p className="help">Override only for a proxy or tunnel. Both <code>https://host/api</code> (local-API shape) and <code>https://host</code> (Web-API shape) are detected automatically.</p>
            </div>
            <div className="field">
              <label className="label is-small">Zotero user ID (optional)</label>
              <input
                className="input"
                value={prefs.zotero_local_user_id ?? ""}
                onChange={(e) => update("zotero_local_user_id", e.target.value)}
                placeholder="0"
                inputMode="numeric"
              />
              <p className="help">Defaults to <code>0</code> (the local Zotero convention). Set to your real Zotero user ID if your proxy answers "Invalid user ID" with 0.</p>
            </div>
            <div className="field">
              <label className="label is-small">Zotero API key (optional)</label>
              <input
                className="input"
                type="password"
                value={prefs.zotero_api_key ?? ""}
                onChange={(e) => update("zotero_api_key", e.target.value)}
                placeholder="P9HzM2x..."
              />
              <p className="help">
                Plain localhost Zotero doesn't need one. Provide if your proxy forwards upstream to <code>api.zotero.org</code> (Zotero replies with "an api key is required"). Get one at{" "}
                <a href="https://www.zotero.org/settings/keys" target="_blank" rel="noreferrer">zotero.org/settings/keys</a>.
              </p>
            </div>
            <p className="is-size-7 has-text-grey mb-2">
              <strong>Reverse-proxy Basic Auth (optional)</strong> — fill these in if your server is behind HTTP Basic Auth (nginx htpasswd, etc.). Distinct from the Zotero API key.
            </p>
            <div className="columns">
              <div className="column">
                <div className="field">
                  <label className="label is-small">Proxy username</label>
                  <input
                    className="input"
                    value={prefs.zotero_basic_auth_user ?? ""}
                    onChange={(e) => update("zotero_basic_auth_user", e.target.value)}
                    placeholder="tucker"
                    autoComplete="off"
                  />
                </div>
              </div>
              <div className="column">
                <div className="field">
                  <label className="label is-small">Proxy password</label>
                  <input
                    className="input"
                    type="password"
                    value={prefs.zotero_basic_auth_pass ?? ""}
                    onChange={(e) => update("zotero_basic_auth_pass", e.target.value)}
                    placeholder="••••••••"
                    autoComplete="off"
                  />
                </div>
              </div>
            </div>
          </>
        )}
        {!prefs.zotero_use_local && (
          <>
            <p className="is-size-7 has-text-grey mb-2">
              Cloud mode — create an API key at{" "}
              <a href="https://www.zotero.org/settings/keys" target="_blank" rel="noreferrer">zotero.org/settings/keys</a>
              {" "}with "Allow library access" and "Allow notes/files access." Your user ID is shown on the same page.
            </p>
            <div className="columns">
              <div className="column">
                <div className="field">
                  <label className="label is-small">User ID</label>
                  <input
                    className="input"
                    value={prefs.zotero_user_id ?? ""}
                    onChange={(e) => update("zotero_user_id", e.target.value)}
                    placeholder="123456"
                    inputMode="numeric"
                  />
                </div>
              </div>
              <div className="column">
                <div className="field">
                  <label className="label is-small">API key</label>
                  <input
                    className="input"
                    type="password"
                    value={prefs.zotero_api_key ?? ""}
                    onChange={(e) => update("zotero_api_key", e.target.value)}
                    placeholder="P9HzM2x..."
                  />
                </div>
              </div>
            </div>
          </>
        )}
      </div>

      <div className="box">
        <h2 className="title is-5">Calendar</h2>
        <div className="field">
          <label className="checkbox">
            <input
              type="checkbox"
              checked={prefs.calendar_show_empty_days}
              onChange={(e) => update("calendar_show_empty_days", e.target.checked)}
            />{" "}
            Show empty-day placeholders
          </label>
          <p className="help">Render a thin separator for each day of the week, even days with no assignments.</p>
        </div>
      </div>

      <div className="box">
        <h2 className="title is-5">Content</h2>
        <div className="field">
          <label className="checkbox">
            <input
              type="checkbox"
              checked={prefs.cache_content}
              onChange={(e) => update("cache_content", e.target.checked)}
            />{" "}
            Cache course content for offline use
          </label>
          <p className="help">
            When on, use each course's <strong>Make available offline</strong> button to store its files,
            Tools, and media on this device. Cached content opens instantly and works offline, and the
            course export reuses it. Quizzes are never cached. Turning this off makes the app fetch
            everything live again (cached files stay on disk until you clear a course).
          </p>
        </div>
      </div>

      <div className="box">
        <h2 className="title is-5">REST API</h2>
        <p className="is-size-7 has-text-grey mb-3">
          Optional embedded HTTP server for external integrations. Off by default. Authenticated with the static key below or a short-lived JWT.
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
            <>
              <span className="ml-3 has-text-success">Running on port {restPort}</span>
              <a
                className="ml-3"
                href={`http://127.0.0.1:${restPort}/docs`}
                target="_blank"
                rel="noreferrer"
              >
                Swagger docs
              </a>
            </>
          )}
        </div>
      </div>

      <div className="box">
        <h2 className="title is-5">Spotify</h2>
        <p className="is-size-7 has-text-grey mb-3">
          Used to read course playlist tracklists (e.g. MUH-105's unit playlists) so they
          can be taken to Apple Music. Metadata only — no audio. Create a free app at{" "}
          <a href="https://developer.spotify.com/dashboard" target="_blank" rel="noreferrer">
            developer.spotify.com/dashboard
          </a>{" "}
          and paste its Client ID + Secret here. Stored on this device only — never synced to
          paired devices.
        </p>
        <div className="field">
          <label className="label is-small">Client ID</label>
          <div className="control">
            <input
              className="input"
              value={prefs.spotify_client_id ?? ""}
              onChange={(e) => update("spotify_client_id", e.target.value)}
              placeholder="e.g. 4f9a1c…"
            />
          </div>
        </div>
        <div className="field">
          <label className="label is-small">Client Secret</label>
          <div className="control">
            <input
              className="input"
              type="password"
              value={prefs.spotify_client_secret ?? ""}
              onChange={(e) => update("spotify_client_secret", e.target.value)}
              placeholder="kept on this device"
            />
          </div>
        </div>
      </div>

      <UpdatesBox />

      <ExportAuthBox />

      <ImportFromOldBrilliantBox />

      <div className="box">
        <h2 className="title is-5">Build info</h2>
        {build ? (
          <table className="table is-narrow is-fullwidth" style={{ background: "transparent" }}>
            <tbody>
              <tr>
                <td className="has-text-grey" style={{ width: "30%" }}>App</td>
                <td>{build.appName} <span className="has-text-grey">v{build.appVersion}</span></td>
              </tr>
              <tr>
                <td className="has-text-grey">Tauri runtime</td>
                <td>{build.tauriVersion}</td>
              </tr>
              {build.devHost && (
                <tr>
                  <td className="has-text-grey">Dev server</td>
                  <td><code>{build.devHost}</code> <span className="tag is-warning is-light ml-2">development build</span></td>
                </tr>
              )}
            </tbody>
          </table>
        ) : (
          <p className="has-text-grey is-size-7">Loading build info…</p>
        )}
      </div>
    </div>
  );
}

function ImportFromOldBrilliantBox() {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  async function run() {
    setBusy(true);
    setErr(null);
    setResult(null);
    try {
      const r = await api.importFromOldBrilliant();
      setResult(
        `Updated ${r.courses_updated} course${r.courses_updated === 1 ? "" : "s"}, ` +
        `${r.assignments_updated} assignment${r.assignments_updated === 1 ? "" : "s"}, ` +
        `inserted ${r.synthetic_assignments_inserted} synthetic task${r.synthetic_assignments_inserted === 1 ? "" : "s"} ` +
        `and ${r.notifications_inserted} historical announcement${r.notifications_inserted === 1 ? "" : "s"}. ` +
        `Profile prefs ${r.prefs_overlaid ? "merged" : "left as-is"}. From ${r.old_db_path}.`
      );
    } catch (e) {
      setErr(String((e as { message?: string })?.message ?? e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="box">
      <h2 className="title is-5">Import from old Brilliant</h2>
      <p className="is-size-7 has-text-grey mb-3">
        One-shot pull from the old Sinatra/Rails Brilliant SQLite database. Brings over
        course customizations (pinned, color, target grade, units, end-of-week, sort order),
        per-assignment state (completion, optional, manual edits), synthetic tasks,
        historical announcements, and profile preferences (display name, GPA history, semester colors,
        collapsed topics). Brightspace-sourced fields are not overwritten — the next sync
        owns them. Safe to run more than once.
      </p>
      <button className={`button is-primary ${busy ? "is-loading" : ""}`} disabled={busy} onClick={run}>
        <span className="icon"><i className="fas fa-file-import"></i></span>
        <span>{busy ? "Importing…" : "Import"}</span>
      </button>
      {result && <p className="help has-text-success-dark mt-3">{result}</p>}
      {err && <p className="help is-danger mt-3">{err}</p>}
    </div>
  );
}

function UpdatesBox() {
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  // undefined = not checked yet; null = up to date; object = update available
  const [update, setUpdate] = useState<UpdateInfo | null | undefined>(undefined);
  const [err, setErr] = useState<string | null>(null);

  async function check() {
    setChecking(true);
    setErr(null);
    try {
      setUpdate(await api.checkForUpdates());
    } catch (e: any) {
      setErr(String(e?.message ?? e));
    } finally {
      setChecking(false);
    }
  }

  async function install() {
    setInstalling(true);
    setErr(null);
    try {
      // On success the app downloads, installs, and restarts — this won't return.
      await api.installUpdate();
    } catch (e: any) {
      setErr(String(e?.message ?? e));
      setInstalling(false);
    }
  }

  return (
    <div className="box">
      <h2 className="title is-5">Software update</h2>
      <p className="is-size-7 has-text-grey mb-3">
        The desktop app updates over-the-air. (On mobile, updates come from the App Store.)
      </p>
      {update && update.version && (
        <div className="notification is-info is-light">
          <strong>Update available: {update.version}</strong> — you have {update.current_version}.
          {update.notes && (
            <p className="is-size-7 mt-1" style={{ whiteSpace: "pre-wrap" }}>{update.notes}</p>
          )}
        </div>
      )}
      {update === null && <p className="has-text-success mb-3">You're on the latest version.</p>}
      {err && <div className="notification is-danger is-light">{err}</div>}
      <div className="field is-grouped">
        <div className="control">
          <button className={`button ${checking ? "is-loading" : ""}`} onClick={check} disabled={checking || installing}>
            <span className="icon"><i className="fas fa-arrows-rotate"></i></span>
            <span>Check for updates</span>
          </button>
        </div>
        {update && update.version && (
          <div className="control">
            <button className={`button is-primary ${installing ? "is-loading" : ""}`} onClick={install} disabled={installing}>
              <span className="icon"><i className="fas fa-download"></i></span>
              <span>Install &amp; restart</span>
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

function ExportAuthBox() {
  const [busy, setBusy] = useState(false);
  const [exported, setExported] = useState<{ host: string; cookie: string } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [copied, setCopied] = useState<"host" | "cookie" | "both" | null>(null);

  async function load() {
    setBusy(true);
    setErr(null);
    setCopied(null);
    try {
      const r = await api.exportAuth();
      setExported(r);
    } catch (e) {
      setErr(String((e as { message?: string })?.message ?? e));
    } finally {
      setBusy(false);
    }
  }

  async function copy(text: string, kind: "host" | "cookie" | "both") {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = text;
      document.body.appendChild(ta);
      ta.select();
      try { document.execCommand("copy"); } catch { /* ignore */ }
      ta.remove();
    }
    setCopied(kind);
    setTimeout(() => setCopied((c) => (c === kind ? null : c)), 2500);
  }

  return (
    <div className="box">
      <h2 className="title is-5">Copy auth to another device</h2>
      <p className="is-size-7 has-text-grey mb-3">
        Faster than re-logging-in on a fresh install: copy the host + session cookie from this
        device, then paste them into Setup → "Brightspace host" and "Session cookies"
        on the new device. The cookie is what authenticates you to Brightspace —
        treat it like a password and don't paste it anywhere public.
      </p>
      {!exported && (
        <button className={`button ${busy ? "is-loading" : ""}`} disabled={busy} onClick={load}>
          <span className="icon"><i className="fas fa-key"></i></span>
          <span>Reveal & copy</span>
        </button>
      )}
      {err && <p className="help is-danger mt-2">{err}</p>}
      {exported && (
        <div className="content is-small">
          <div className="field">
            <label className="label is-small">Brightspace host</label>
            <div className="is-flex" style={{ gap: 6 }}>
              <input className="input" readOnly value={exported.host} />
              <button className={`button is-small ${copied === "host" ? "is-success" : ""}`} onClick={() => copy(exported.host, "host")}>
                <span className="icon is-small"><i className={`fas ${copied === "host" ? "fa-check" : "fa-copy"}`}></i></span>
                <span>{copied === "host" ? "Copied" : "Copy"}</span>
              </button>
            </div>
          </div>
          <div className="field">
            <label className="label is-small">Session cookies</label>
            <textarea
              className="textarea"
              rows={3}
              readOnly
              value={exported.cookie}
              style={{ fontFamily: "monospace", fontSize: "0.7rem", wordBreak: "break-all" }}
            />
            <button className={`button is-small mt-2 ${copied === "cookie" ? "is-success" : ""}`} onClick={() => copy(exported.cookie, "cookie")}>
              <span className="icon is-small"><i className={`fas ${copied === "cookie" ? "fa-check" : "fa-copy"}`}></i></span>
              <span>{copied === "cookie" ? "Copied" : "Copy cookies"}</span>
            </button>
          </div>
          <button className="button is-small is-light" onClick={() => { setExported(null); setCopied(null); }}>
            Hide
          </button>
        </div>
      )}
    </div>
  );
}
