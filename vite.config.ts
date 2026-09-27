import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// Tailwind v4 is configured in CSS (`src/index.css`), not in a
// `tailwind.config.js`. There is deliberately no such file in this repo.
export default defineConfig({
  plugins: [react(), tailwindcss()],

  // Keep in sync with `compilerOptions.paths` in tsconfig.json.
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },

  // Tauri drives this dev server, so the port is fixed and failures must be
  // loud rather than silently landing on another port.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Rust rebuilds are cargo's job; watching target/ would thrash Vite.
      ignored: ["**/src-tauri/**", "**/target/**"],
    },
  },

  // Only `VITE_`- and `TAURI_`-prefixed vars reach the webview.
  envPrefix: ["VITE_", "TAURI_ENV_"],

  build: {
    // WKWebView on the macOS 14.4 floor (SPEC L2).
    target: "safari17",
    sourcemap: true,
  },

  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
  },
});
