import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 期望固定端口，且不能在端口被占用时静默换端口
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  // noVNC 使用 top-level await：依赖预构建与产物转译都必须按 es2022 处理
  esbuild: {
    target: "es2022",
  },
  optimizeDeps: {
    esbuildOptions: {
      target: "es2022",
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    // noVNC 内部使用 top-level await，需要 es2022 及以上
    target: "es2022",
    minify: "esbuild",
    sourcemap: false,
  },
});
