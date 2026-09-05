import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath, URL } from "node:url";

const html = (name: string): string => fileURLToPath(new URL(`./${name}.html`, import.meta.url));

// M4 拆窗：MPA 多入口 —— 每个窗口一个 html（desktop + 四款软件），独立 bundle。
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    target: "chrome110",
    minify: "esbuild",
    sourcemap: false,
    rollupOptions: {
      input: {
        desktop: html("desktop"),
        "app-write": html("app-write"),
        "app-mind": html("app-mind"),
        "app-code": html("app-code"),
        "app-fate": html("app-fate"),
        explorer: html("explorer"),
      },
      output: {
        // 性能：vendor 与图标库拆为稳定命名 chunk —— 跨窗口共享缓存、
        // 业务代码改动不再使整包 971KB 失效，首屏解析明显提速。
        manualChunks: {
          "vendor-react": ["react", "react-dom", "scheduler"],
          "vendor-icons": ["lucide-react"],
        },
      },
    },
  },
});
