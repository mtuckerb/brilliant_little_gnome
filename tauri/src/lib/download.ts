// Helpers for triggering a file download from a base64 payload returned by a
// `#[tauri::command]`. Mirrors the pattern in `SyllabusPanel`.

export interface DownloadPayload {
  bytes_base64?: string | null;
  mime: string | null;
  filename: string;
  saved_path?: string | null;
}

// Re-exported under the more accurate "result" name used across the app since
// Rust now always writes to disk and returns saved_path.
export type DownloadResult = DownloadPayload;

function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function triggerDownload(payload: DownloadPayload) {
  if (payload.saved_path) {
    console.info(`Downloaded ${payload.filename} to ${payload.saved_path}`);
    return;
  }

  if (!payload.bytes_base64) {
    throw new Error(`Download did not return bytes or a saved path for ${payload.filename}`);
  }

  const bytes = base64ToBytes(payload.bytes_base64);
  // Slice copies into a fresh ArrayBuffer (not SharedArrayBuffer) so the
  // BlobPart type-check is happy in strict mode.
  const blob = new Blob([bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer], {
    type: payload.mime || "application/octet-stream",
  });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = payload.filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  // Revoke late so the browser has a chance to start the download.
  setTimeout(() => URL.revokeObjectURL(url), 4000);
}
