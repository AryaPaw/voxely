import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const root = path.resolve(__dirname, "../..");

export default defineConfig({
  plugins: [
    {
      name: "readme-shot-overlay-wallpaper",
      transformIndexHtml(html) {
        return html.replace(
          "<head>",
          `<head>
    <style>
      html.shot-overlay,
      html.shot-overlay body,
      html.shot-overlay #root {
        background:
          radial-gradient(1200px 480px at 50% 20%, rgb(36 52 78), transparent 62%),
          linear-gradient(180deg, rgb(18 22 30), rgb(10 12 16)) !important;
      }
    </style>
    <script>
      if (location.search.includes("overlay")) {
        document.documentElement.classList.add("shot-overlay");
      }
    </script>`,
        );
      },
    },
    react(),
    tailwindcss(),
  ],
  resolve: {
    alias: {
      "@": path.resolve(root, "src"),
      "@tauri-apps/api/core": path.resolve(__dirname, "tauri-core.ts"),
      "@tauri-apps/api/event": path.resolve(__dirname, "tauri-event.ts"),
      "@tauri-apps/api/app": path.resolve(__dirname, "tauri-app.ts"),
    },
  },
  publicDir: path.resolve(root, "public"),
  root,
  clearScreen: false,
  server: {
    port: 1425,
    strictPort: true,
    host: "127.0.0.1",
  },
});
