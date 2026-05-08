/**
 * Storybook preview - global decorators, parameters, and theme/density toolbar.
 *
 * The design CSS is imported verbatim from the desktop app so stories render
 * inside the same token system as the live product. The toolbar globals
 * `theme` and `density` toggle the same `body.light` / `body.compact`
 * classes the running app uses.
 */
import { useEffect } from "react";
import type { Decorator, Preview } from "@storybook/react-vite";

import "@/styles/global.css";

const applyBodyClasses: Decorator = (Story, context) => {
  const theme = (context.globals["theme"] as string | undefined) ?? "dark";
  const density = (context.globals["density"] as string | undefined) ?? "comfortable";

  useEffect(() => {
    const body = document.body;
    body.classList.toggle("light", theme === "light");
    body.classList.toggle("compact", density === "compact");
    return () => {
      body.classList.remove("light");
      body.classList.remove("compact");
    };
  }, [theme, density]);

  return Story();
};

const preview: Preview = {
  parameters: {
    layout: "fullscreen",
    backgrounds: { disable: true },
    controls: { matchers: { color: /(background|color)$/i, date: /Date$/i } },
  },
  globalTypes: {
    theme: {
      description: "Active app theme (toggles body.light)",
      defaultValue: "dark",
      toolbar: {
        title: "Theme",
        icon: "circlehollow",
        items: [
          { value: "dark", title: "Dark", icon: "circle" },
          { value: "light", title: "Light", icon: "circlehollow" },
        ],
        dynamicTitle: true,
      },
    },
    density: {
      description: "Row density (toggles body.compact)",
      defaultValue: "comfortable",
      toolbar: {
        title: "Density",
        icon: "menu",
        items: [
          { value: "comfortable", title: "Comfortable" },
          { value: "compact", title: "Compact" },
        ],
        dynamicTitle: true,
      },
    },
  },
  decorators: [applyBodyClasses],
};

export default preview;
