/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

const host = process.env.TAURI_DEV_HOST;
const projectDir = import.meta.dirname;

export default defineConfig({
  plugins: [react(), tailwindcss()],
  base: "./",
  resolve: {
    alias: {
      "@": path.resolve(projectDir, "./src"),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    rollupOptions: {
      input: {
        main: path.resolve(projectDir, "index.html"),
        overlay: path.resolve(projectDir, "overlay.html"),
      },
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    coverage: {
      provider: "v8",
      reporter: ["text", "lcov"],
      thresholds: {
        statements: 80,
        branches: 85,
        functions: 80,
        lines: 85,
        "src/windows/overlay/OverlayApp.tsx": {
          lines: 85,
          branches: 85,
        },
        "src/windows/main/StatisticsPane.tsx": {
          lines: 85,
          branches: 85,
        },
      },
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["**/*.test.*", "src/main.tsx", "src/overlay.tsx"],
    },
  },
});
