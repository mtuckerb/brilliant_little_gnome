import { useEffect, useMemo, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { api } from "../api";
import { extensionForFile, objectUrlFromBase64, routeFileToViewer } from "../lib/fileViewer";
import { extractOfficeText } from "../lib/officeText";
import { triggerDownload } from "../lib/download";

interface ViewerState {
  url?: string | null;
  name?: string | null;
  size?: number | null;
}

export default function FileViewer() {
  const navigate = useNavigate();
  const location = useLocation();
  const state = (location.state ?? {}) as ViewerState;
  const sourceUrl = state.url ?? "";
  const filename = state.name || "Attachment";
  const route = useMemo(() => routeFileToViewer(filename || sourceUrl, state.size), [filename, sourceUrl, state.size]);
  const [objectUrl, setObjectUrl] = useState<string | null>(null);
  const [payload, setPayload] = useState<{ bytes_base64: string; mime: string | null; filename: string } | null>(null);
  const [text, setText] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let revoke: (() => void) | null = null;
    let cancelled = false;
    async function load() {
      setLoading(true);
      setErr(null);
      setText(null);
      setObjectUrl(null);
      setPayload(null);
      try {
        if (!sourceUrl) throw new Error("Attachment URL is missing.");
        if (!route.supported) throw new Error(route.reason === "too_large" ? "File too large to preview." : "File type is not supported for preview.");
        const next = await api.previewAttachment(sourceUrl, filename);
        if (cancelled) return;
        setPayload(next);
        const created = objectUrlFromBase64(next.bytes_base64, next.mime);
        revoke = created.revoke;
        setObjectUrl(created.url);
        if (route.kind === "officeXml") {
          setText(extractOfficeText(created.bytes, extensionForFile(filename)) || "No readable text found in this Office document.");
        } else if (["text", "markdown", "csv", "rtf"].includes(route.kind)) {
          setText(new TextDecoder().decode(created.bytes));
        }
      } catch (e) {
        if (!cancelled) setErr(String((e as { message?: string })?.message ?? e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    }
    load();
    return () => {
      cancelled = true;
      revoke?.();
    };
  }, [sourceUrl, filename, route.kind, route.reason, route.supported]);

  function openExternally() {
    if (payload) triggerDownload(payload);
    else if (sourceUrl) window.open(sourceUrl, "_blank", "noreferrer");
  }

  return (
    <div>
      <nav className="level mb-3">
        <div className="level-left"><h1 className="title is-5 mb-0">{filename}</h1></div>
        <div className="level-right buttons">
          <button className="button" onClick={() => navigate(-1)}>Close</button>
          <button className="button is-link is-light" onClick={openExternally}>Open externally</button>
        </div>
      </nav>

      {loading && <div className="has-text-centered py-6"><span className="icon has-text-primary"><i className="fas fa-circle-notch fa-spin"></i></span><span className="ml-2">Loading preview…</span></div>}

      {!loading && err && (
        <div className="notification is-warning">
          <p className="mb-3">{err}</p>
          <div className="buttons">
            <button className="button is-primary" onClick={() => window.location.reload()}>Try again</button>
            <button className="button" onClick={openExternally}>Open externally</button>
          </div>
        </div>
      )}

      {!loading && !err && objectUrl && route.kind === "native" && (
        <iframe title={filename} src={objectUrl} style={{ width: "100%", height: "75vh", border: "1px solid #ddd", borderRadius: 6 }} />
      )}

      {!loading && !err && text != null && route.kind !== "native" && (
        <pre className="box" style={{ whiteSpace: "pre-wrap", overflow: "auto", maxHeight: "75vh" }}>{text}</pre>
      )}
    </div>
  );
}
