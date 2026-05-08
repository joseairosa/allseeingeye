/**
 * Storybook 9 main config.
 *
 * The framework is React + Vite. We extend the Vite resolve aliases so
 * stories can import live components from `apps/desktop/src` via the same
 * `@/*` path the running app uses; the design CSS imports therefore work
 * without duplication.
 */
import path from "node:path";
import { fileURLToPath } from "node:url";
import type { StorybookConfig } from "@storybook/react-vite";
import { mergeConfig } from "vite";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../../..");
const desktopSrc = path.resolve(repoRoot, "apps/desktop/src");
const sharedTypesSrc = path.resolve(repoRoot, "packages/shared-types/src");

const config: StorybookConfig = {
  framework: "@storybook/react-vite",
  stories: ["../stories/**/*.stories.@(ts|tsx)"],
  addons: ["@storybook/addon-themes"],
  core: {
    disableTelemetry: true,
  },
  typescript: {
    check: false,
  },
  async viteFinal(viteConfig) {
    return mergeConfig(viteConfig, {
      resolve: {
        alias: {
          "@": desktopSrc,
          "@aseye/shared-types": sharedTypesSrc,
        },
      },
      server: {
        fs: {
          // Allow Storybook's Vite dev server to read files from the desktop
          // workspace and the static design assets in apps/desktop/public.
          allow: [repoRoot],
        },
      },
    });
  },
  staticDirs: [path.resolve(repoRoot, "apps/desktop/public")],
};

export default config;
