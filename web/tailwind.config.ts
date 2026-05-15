// Tailwind v4 reads its theme tokens directly from CSS (`@theme` in
// `src/styles/globals.css`), so this file is intentionally minimal.
// It exists so editors and ts-driven tooling can resolve a Tailwind
// project root.
import type { Config } from "tailwindcss";

const config: Config = {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
};

export default config;
