import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const root = path.resolve(import.meta.dirname, "../..");
if (!process.env.VOXELY_SHOTS_WORK) throw new Error("Run bun run docs:shots instead");

export default defineConfig({
  root,
  plugins: [
    react(),
    tailwindcss(),
    {
      name: "native-readme-driver",
      transformIndexHtml: {
        order: "pre",
        handler: (html) =>
          html.replace(
            /src="\/src\/(main|overlay)\.tsx"/,
            'src="/scripts/readme-shots/native-driver.ts"',
          ),
      },
    },
  ],
  resolve: {
    alias: {
      "@": path.join(root, "src"),
      "@tauri-apps/api/core": path.join(import.meta.dirname, "native-core.ts"),
      "@tauri-apps/api/event": path.join(import.meta.dirname, "native-event.ts"),
      "@tauri-apps/api/app": path.join(import.meta.dirname, "tauri-app.ts"),
    },
  },
  build: {
    outDir: path.join(process.env.VOXELY_SHOTS_WORK, "assets"),
    emptyOutDir: false,
    rollupOptions: {
      input: { main: path.join(root, "index.html"), overlay: path.join(root, "overlay.html") },
    },
  },
});
