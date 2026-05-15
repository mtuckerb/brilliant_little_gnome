import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const remoteDevHost = process.env.BRILLIANT_TAURI_REMOTE_DEV === "1"
  ? process.env.TAURI_DEV_HOST
  : undefined;

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
