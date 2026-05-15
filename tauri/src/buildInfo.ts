export const IS_DEV_BUILD = import.meta.env.DEV;

export function devServerHost(): string | null {
  if (!IS_DEV_BUILD) return null;

  const host = window.location.host;
  return host || null;
}
