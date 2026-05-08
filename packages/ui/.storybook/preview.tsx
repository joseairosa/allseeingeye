/**
 * Storybook preview - global decorators, parameters, and theme/density toolbar.
 *
 * The design CSS is imported verbatim from the desktop app so stories render
 * inside the same token system as the live product. The toolbar globals
 * `theme` and `density` toggle the same `body.light` / `body.compact`
 * classes the running app uses.
 *
 * A `QueryClientProvider` wraps every story since Phase 2.1 components
 * (Sidebar, QuickLook, InventoryView) consume `useQuery` hooks. Stories
 * that want live-looking data seed the cache via the helpers in
 * `./query-fixtures.ts`.
 */
import { useEffect } from "react";
import type { Decorator, Preview } from "@storybook/react-vite";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import "@/styles/global.css";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Stories pre-seed the cache - we don't want background refetches
      // hitting `invoke()` and failing outside the Tauri host.
      retry: false,
      staleTime: Infinity,
      gcTime: Infinity,
      refetchOnWindowFocus: false,
      refetchOnMount: false,
      refetchOnReconnect: false,
    },
  },
});

const wrapWithQueryClient: Decorator = (Story) => (
  <QueryClientProvider client={queryClient}>{Story()}</QueryClientProvider>
);

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
  decorators: [wrapWithQueryClient, applyBodyClasses],
};

export { queryClient };

export default preview;
