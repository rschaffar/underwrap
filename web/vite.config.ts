import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

// The backend admin listener defaults to https://localhost:9443 on the
// developer's machine; the dev server proxies /v1/admin/* to it so the
// SPA can call relative URLs in both dev and production.
const PROXY_TARGET =
  process.env.UNDERWRAP_BACKEND_DEV_URL ?? "http://127.0.0.1:9080";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  build: {
    outDir: "dist",
    sourcemap: false,
    // Embedded into the Rust binary via rust-embed; keep emitted
    // filenames stable and hashed for cache-busting at the asset
    // level.
    rollupOptions: {
      output: {
        entryFileNames: "assets/[name]-[hash].js",
        chunkFileNames: "assets/[name]-[hash].js",
        assetFileNames: "assets/[name]-[hash][extname]",
      },
    },
  },
  server: {
    port: 5173,
    proxy: {
      "/v1": {
        target: PROXY_TARGET,
        changeOrigin: true,
        secure: false,
      },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test-setup.ts"],
    css: true,
  },
});
