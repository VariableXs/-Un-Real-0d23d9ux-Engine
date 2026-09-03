import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // Allow dev-server embedding under preview/proxy hosts (remote sandboxes,
    // codespaces etc.). Does not affect the packaged Tauri app.
    allowedHosts: true,
  },
  build: {
    target: "chrome110",
    minify: "esbuild",
    sourcemap: false,
  },
});
