export const PREVIEW_MAX_BYTES = 25 * 1024 * 1024;

export const SUPPORTED_VIEWER_EXTENSIONS = [
  "pdf",
  "doc",
  "docx",
  "xls",
  "xlsx",
  "csv",
  "md",
  "markdown",
  "ppt",
  "pptx",
  "rtf",
] as const;

export type SupportedViewerExtension = (typeof SUPPORTED_VIEWER_EXTENSIONS)[number];
export type ViewerKind = "native" | "legacyOffice" | "text" | "markdown" | "csv" | "rtf" | "officeXml" | "external";

export interface ViewerRoute {
  supported: boolean;
  extension: string;
  kind: ViewerKind;
  reason?: string;
}

export function extensionForFile(nameOrUrl: string): string {
  const withoutQuery = nameOrUrl.split(/[?#]/, 1)[0] ?? nameOrUrl;
  const last = withoutQuery.split("/").pop() ?? withoutQuery;
  const match = /\.([a-z0-9]+)$/i.exec(last.trim());
  return match ? match[1].toLowerCase() : "";
}

export function routeFileToViewer(nameOrUrl: string, sizeBytes?: number | null): ViewerRoute {
  const extension = extensionForFile(nameOrUrl);
  if (sizeBytes != null && sizeBytes > PREVIEW_MAX_BYTES) {
    return { supported: false, extension, kind: "external", reason: "too_large" };
  }

  switch (extension) {
    case "pdf":
      return { supported: true, extension, kind: "native" };
    case "md":
    case "markdown":
      return { supported: true, extension, kind: "markdown" };
    case "csv":
      return { supported: true, extension, kind: "csv" };
    case "rtf":
      return { supported: true, extension, kind: "rtf" };
    case "docx":
    case "xlsx":
    case "pptx":
      return { supported: true, extension, kind: "officeXml" };
    case "doc":
    case "xls":
    case "ppt":
      // Legacy Office binaries are not reliably rendered by mobile WebViews.
      // Route them to the in-app viewer so users get a titled screen and a clear
      // Open externally fallback instead of silently downloading/handing off.
      return { supported: true, extension, kind: "legacyOffice" };
    default:
      return { supported: false, extension, kind: "external", reason: "unsupported" };
  }
}

export function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i += 1) out[i] = bin.charCodeAt(i);
  return out;
}

export function objectUrlFromBase64(bytesBase64: string, mime: string | null) {
  const bytes = base64ToBytes(bytesBase64);
  const blob = new Blob([bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer], {
    type: mime || "application/octet-stream",
  });
  const url = URL.createObjectURL(blob);
  return { url, revoke: () => URL.revokeObjectURL(url), bytes };
}
