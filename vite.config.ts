import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => {
  const isDebug = !!process.env.TAURI_ENV_DEBUG;

  return {
    plugins: [react()],

    // Tauri expects a fixed port and fails if it is not available.
    clearScreen: false,
    server: {
      port: 1420,
      strictPort: true,
      host: host || false,
      hmr: host
        ? {
            protocol: "ws",
            host,
            port: 1421,
          }
        : undefined,
      watch: {
        // Rust sources are watched by cargo, not vite.
        ignored: ["**/src-tauri/**"],
      },
    },

    envPrefix: ["VITE_", "TAURI_ENV_"],

    build: {
      // Tauri uses WKWebView on macOS / WebView2 on Windows.
      target: "safari15",
      // 开发构建保留可读性与 sourcemap；发布构建交给 vite 的默认压缩器。
      ...(isDebug ? { minify: false as const, sourcemap: true } : {}),
    },
  };
});
