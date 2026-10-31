import { defineConfig } from "vite";
import { resolve } from "path";

// Each part of the UI layer (dock, top bar, launcher, ...) is its own HTML page shown
// in its own Tauri window, but they all live in one app process to keep memory low.
export default defineConfig({
  root: "src",
  publicDir: resolve(__dirname, "assets"),
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: {
    outDir: resolve(__dirname, "dist"),
    emptyOutDir: true,
    target: "chrome110",
    rollupOptions: {
      input: {
        dock: resolve(__dirname, "src/dock.html"),
        topbar: resolve(__dirname, "src/topbar.html"),
        settings: resolve(__dirname, "src/settings.html"),
        widgets: resolve(__dirname, "src/widgets.html"),
      },
    },
  },
});
