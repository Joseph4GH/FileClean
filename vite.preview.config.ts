import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

/**
 * 无后端 UI 预览配置（`npm run preview:ui`）。
 *
 * 与主配置的唯一区别是端口与入口：主配置绑定 1420 给 `tauri dev`，
 * 这里用 4175 + `preview.html`（由 `src/preview.tsx` 用官方 mocks 顶替 IPC）。
 * 只用于看版式，不参与打包产物。
 */
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 4175,
    strictPort: true,
    host: false,
  },
  build: {
    target: "safari15",
    outDir: "dist-preview",
    // 必须显式指定 input：Vite 默认只把根目录的 `index.html` 当作入口，
    // 那样 `vite build --config vite.preview.config.ts` 会打出与正式产物
    // **完全相同**的 bundle（连文件名哈希都一样），预览入口根本没被编译 ——
    // 「预览可构建」就成了假阳性。
    rollupOptions: {
      input: "preview.html",
    },
  },
});
