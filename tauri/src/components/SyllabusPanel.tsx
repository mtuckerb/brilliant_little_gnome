import { useEffect, useState } from "react";
import { api } from "../api";

interface Props {
  courseId: string;
}

interface OverviewMeta {
  description_html: string | null;
  has_attachment: boolean;
  attachment_name: string | null;
  attachment_url: string | null;
}

// Decode a base64 string from the backend into a same-origin Blob URL the
// webview can render in <iframe>/<a download>. Browsers won't let us hand a
// remote Brightspace URL to <iframe> because of cookie scoping, so we proxy the
// bytes through the Rust client and rebuild a Blob locally.
function blobFromBase64(b64: string, mime: string | null): { url: string; revoke: () => void } {
  const binary = atob(b64);
  const arr = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) arr[i] = binary.charCodeAt(i);
  const blob = new Blob([arr], { type: mime ?? "application/octet-stream" });
  const url = URL.createObjectURL(blob);
  return { url, revoke: () => URL.revokeObjectURL(url) };
}

export default function SyllabusPanel({ courseId }: Props) {
  const [meta, setMeta] = useState<OverviewMeta | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState<"view" | "download" | null>(null);
  const [viewerUrl, setViewerUrl] = useState<string | null>(null);
  const [viewerName, setViewerName] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .getCourseOverview(courseId)
      .then((m) => { if (!cancelled) setMeta(m); })
      .catch((e) => { if (!cancelled) setErr(String(e?.message ?? e)); });
    return () => {
      cancelled = true;
    };
  }, [courseId]);

  // Revoke any blob URL when the viewer closes or component unmounts.
  useEffect(() => () => { if (viewerUrl) URL.revokeObjectURL(viewerUrl); }, [viewerUrl]);

  async function handleView() {
    if (!meta) return;
    setBusy("view");
    setErr(null);
    try {
      if (meta.has_attachment && meta.attachment_url) {
        const att = await api.fetchCourseOverviewAttachment(courseId);
        const { url } = blobFromBase64(att.bytes_base64, att.mime);
        setViewerUrl(url);
        setViewerName(att.filename);
      } else if (meta.description_html) {
        // Wrap raw HTML in a self-contained doc so any relative links don't
        // try to resolve against the Tauri custom protocol.
        const html = `<!doctype html><html><head><meta charset="utf-8"><base target="_blank"><style>body{font-family:sans-serif;padding:24px;max-width:800px;margin:0 auto;line-height:1.5;}</style></head><body>${meta.description_html}</body></html>`;
        const blob = new Blob([html], { type: "text/html" });
        const url = URL.createObjectURL(blob);
        setViewerUrl(url);
        setViewerName("Syllabus");
      } else {
        setErr("This course has no syllabus or overview yet.");
      }
    } catch (e) {
      setErr(String((e as { message?: string })?.message ?? e));
    } finally {
      setBusy(null);
    }
  }

  async function handleDownload() {
    if (!meta?.has_attachment) return;
    setBusy("download");
    setErr(null);
    try {
      const att = await api.fetchCourseOverviewAttachment(courseId);
      const { url, revoke } = blobFromBase64(att.bytes_base64, att.mime);
      const a = document.createElement("a");
      a.href = url;
      a.download = att.filename;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      // Defer revoke; some browsers race the download otherwise.
      setTimeout(revoke, 4000);
    } catch (e) {
      setErr(String((e as { message?: string })?.message ?? e));
    } finally {
      setBusy(null);
    }
  }

  if (err && !meta) {
    // Most courses without an overview return 404 from Brightspace; treat that
    // as a soft "no syllabus" rather than a loud error banner.
    return (
      <div className="box has-text-grey-light">
        <i className="fas fa-file-alt mr-2"></i>No syllabus available.
      </div>
    );
  }
  if (!meta) {
    return (
      <div className="box has-text-grey-light">
        <i className="fas fa-circle-notch fa-spin mr-2"></i>Checking syllabus…
      </div>
    );
  }

  const nothing = !meta.has_attachment && !meta.description_html;

  return (
    <div className="box">
      <div className="level mb-0">
        <div className="level-left">
          <h2 className="title is-6 mb-0"><i className="fas fa-file-alt mr-2 has-text-grey"></i>Syllabus / Overview</h2>
        </div>
        <div className="level-right">
          <button className="button is-small is-primary mr-2" disabled={nothing || busy !== null} onClick={handleView}>
            <span className="icon"><i className="fas fa-eye"></i></span>
            <span>{busy === "view" ? "Loading…" : "View"}</span>
          </button>
          <button className="button is-small" disabled={!meta.has_attachment || busy !== null} onClick={handleDownload}>
            <span className="icon"><i className="fas fa-download"></i></span>
            <span>{busy === "download" ? "Saving…" : "Download"}</span>
          </button>
        </div>
      </div>
      {nothing && <p className="has-text-grey is-size-7 mt-2">This course has no syllabus or overview yet.</p>}
      {meta.has_attachment && meta.attachment_name && (
        <p className="is-size-7 has-text-grey mt-2">Attached file: {meta.attachment_name}</p>
      )}
      {err && <p className="help is-danger mt-2">{err}</p>}

      {viewerUrl && (
        <div className="modal is-active">
          <div className="modal-background" onClick={() => setViewerUrl(null)} />
          <div className="modal-card" style={{ width: "min(90vw, 1100px)", height: "85vh" }}>
            <header className="modal-card-head">
              <p className="modal-card-title">{viewerName ?? "Syllabus"}</p>
              <button className="delete" aria-label="close" onClick={() => setViewerUrl(null)} />
            </header>
            <section className="modal-card-body" style={{ padding: 0 }}>
              <iframe
                src={viewerUrl}
                title={viewerName ?? "Syllabus"}
                style={{ width: "100%", height: "100%", border: 0 }}
              />
            </section>
          </div>
        </div>
      )}
    </div>
  );
}
