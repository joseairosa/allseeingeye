import { defineConfig, type UserConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";
import { readFileSync } from "node:fs";

const host = process.env.TAURI_DEV_HOST;

// Read the desktop package.json once at config load. The version is
// surfaced to the WebView as `__APP_VERSION__` so the Diagnostics panel
// can show a single source of truth without duplicating the literal.
const pkg = JSON.parse(
  readFileSync(path.resolve(__dirname, "./package.json"), "utf8"),
) as { version: string };

const serverHmr = host
  ? { protocol: "ws" as const, host, port: 1421 }
  : undefined;

// https://vite.dev/config
export default defineConfig((): UserConfig => ({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
      "@aseye/shared-types": path.resolve(__dirname, "../../packages/shared-types/src"),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    ...(host ? { host } : {}),
    ...(serverHmr ? { hmr: serverHmr } : {}),
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  build: {
    target: process.env["TAURI_ENV_PLATFORM"] === "windows" ? "chrome105" : "safari13",
    minify: process.env["TAURI_ENV_DEBUG"] ? false : "esbuild",
    sourcemap: Boolean(process.env["TAURI_ENV_DEBUG"]),
  },
}));
