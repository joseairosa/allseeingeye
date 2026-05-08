/// <reference types="vite/client" />

/**
 * Build-time constant injected by `vite.config.ts::define`. Resolves to
 * the desktop workspace's package.json `version` so the Diagnostics
 * panel renders a single source of truth.
 */
declare const __APP_VERSION__: string;
