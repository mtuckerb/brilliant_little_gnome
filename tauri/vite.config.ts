import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// `tauri ios dev --host` sets TAURI_DEV_HOST to the Mac's LAN IP so the
// phone can reach the dev server. We just honor that whenever it's set —
// no separate opt-in flag needed.
const remoteDevHost = process.env.TAURI_DEV_HOST || undefined;

export default defineConfig(async () => ({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: remoteDevHost || "127.0.0.1",
    hmr: remoteDevHost
      ? { protocol: "ws", host: remoteDevHost, port: 1421 }
      : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
}));
