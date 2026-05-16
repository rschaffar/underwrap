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
    // jsdom enforces the `Secure` requirement on `__Host-` cookies,
    // which is only set over https. Pin the jsdom URL to https so
    // tests exercise the same cookie shape the SPA sees in
    // production.
    environmentOptions: {
      jsdom: {
        url: "https://underwrap.test/",
      },
    },
    globals: true,
    setupFiles: ["./src/test-setup.ts"],
    css: true,
    // Coverage gates only the small libraries the SPA depends on for
    // correctness: the API client and the error/URL helpers in
    // `lib/`. Component coverage is exercised by RTL + Playwright;
    // gating it here would force us into shallow snapshot tests.
    coverage: {
      provider: "v8",
      reporter: ["text", "html"],
      include: ["src/api/**/*.ts", "src/lib/**/*.ts"],
      exclude: ["src/api/types/**", "**/*.test.*"],
      thresholds: {
        lines: 80,
        statements: 80,
        functions: 80,
        branches: 80,
      },
    },
    // Playwright specs live under `web/e2e/` and run via
    // `playwright test`, not vitest. Exclude them so vitest doesn't
    // try to import them as unit tests.
    exclude: ["node_modules/**", "dist/**", "e2e/**", ".direnv/**"],
  },
});
