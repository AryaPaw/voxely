/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
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
        main: path.resolve(__dirname, "index.html"),
        overlay: path.resolve(__dirname, "overlay.html"),
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
        lines: 85,
        branches: 85,
      },
      include: [
        "src/lib/session-copy.ts",
        "src/lib/i18n.ts",
        "src/lib/utils.ts",
        "src/lib/theme.ts",
        "src/lib/history-sync.ts",
        "src/lib/overlay-wave.ts",
        "src/lib/dsp-level.ts",
        "src/components/settings/SettingsField.tsx",
        "src/components/settings/SettingsSwitchRow.tsx",
        "src/components/ui/button.tsx",
        "src/components/ui/input.tsx",
        "src/components/ui/label.tsx",
        "src/components/ui/switch.tsx",
        "src/components/ui/simple-select.tsx",
        "src/components/ui/select.tsx",
        "src/windows/main/sections/AppearanceSettings.tsx",
        "src/windows/main/sections/AboutSettings.tsx",
        "src/windows/main/sections/AdvancedSettings.tsx",
        "src/windows/overlay/OverlayApp.tsx",
        "src/components/settings/SettingsSwitchRow.tsx",
        "src/components/ui/button.tsx",
        "src/components/ui/input.tsx",
        "src/components/ui/label.tsx",
        "src/components/ui/switch.tsx",
        "src/components/ui/simple-select.tsx",
        "src/components/ui/select.tsx",
      ],
      exclude: ["**/*.test.*"],
    },
  },
});
