/**
 * Shared story wrapper.
 *
 * The design CSS expects components to live inside an `.app-shell` grid;
 * isolated stories would otherwise render with no width/height and the
 * sidebar / titlebar would collapse. This wrapper recreates the minimal
 * grid scaffold so a single component renders in its real surface.
 *
 * `mode="full"` renders the entire app shell (titlebar, sidebar, main).
 * `mode="solo"` puts the child inside `.app-shell` only - useful for
 * floating chrome (palette, quick look, onboarding).
 */
import type { ReactNode } from "react";

interface ShellProps {
  children: ReactNode;
  mode?: "solo" | "full";
}

export function Shell({ children, mode = "solo" }: ShellProps) {
  if (mode === "full") {
    return <div className="app-shell">{children}</div>;
  }
  return (
    <div className="app-shell" style={{ minHeight: "100vh" }}>
      {children}
    </div>
  );
}
