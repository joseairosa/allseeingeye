/**
 * Reusable inline SVG icon components.
 *
 * Type-icons (skill / agent / command / mcp / rule / memory / hook) come
 * from `public/assets/type-icons.svg` via <use href>. Other icons are
 * inline so we can apply currentColor without a separate sprite.
 */
import type { SVGProps } from "react";

export type TypeIconId =
  | "icon-skill"
  | "icon-agent"
  | "icon-command"
  | "icon-mcp"
  | "icon-rule"
  | "icon-memory"
  | "icon-hook";

interface TypeIconProps extends SVGProps<SVGSVGElement> {
  id: TypeIconId;
}

export function TypeIcon({ id, className = "type-icon", ...rest }: TypeIconProps) {
  return (
    <svg className={className} aria-hidden="true" {...rest}>
      <use href={`/assets/type-icons.svg#${id}`} />
    </svg>
  );
}

export function CloseIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M6 6l12 12M18 6 6 18" />
    </svg>
  );
}

export function SearchIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <circle cx="11" cy="11" r="7" />
      <path d="m16.5 16.5 4 4" />
    </svg>
  );
}

export function CommandSearchIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M10 18a8 8 0 1 1 5.3-14" />
      <path d="m14 14 6 6" />
    </svg>
  );
}

export function RefreshIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M20 6v5h-5" />
      <path d="M4 18v-5h5" />
      <path d="M17.7 9A6 6 0 0 0 7 6.3L4 9" />
      <path d="M6.3 15A6 6 0 0 0 17 17.7L20 15" />
    </svg>
  );
}

export function FiltersIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M4 6h16M7 12h10M10 18h4" />
    </svg>
  );
}

export function DensityIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M4 7h16M4 12h16M4 17h16" />
    </svg>
  );
}

export function ThemeIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M12 3a6.8 6.8 0 0 0 0 13.6A7.2 7.2 0 0 1 12 3Z" />
      <path d="M19 12.4A7 7 0 1 1 11.6 5" />
    </svg>
  );
}

export function NavInventoryIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M4 5h16M4 12h16M4 19h16" />
    </svg>
  );
}

export function NavMapIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <circle cx="6" cy="7" r="2" />
      <circle cx="18" cy="7" r="2" />
      <circle cx="12" cy="18" r="2" />
      <path d="m8 8 3 8M16 8l-3 8M8 7h8" />
    </svg>
  );
}

export function NavEditorIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M5 4h14v16H5z" />
      <path d="M8 8h8M8 12h6M8 16h5" />
    </svg>
  );
}

export function NavHealthIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M4 13h4l2-6 4 12 2-6h4" />
    </svg>
  );
}

/**
 * Coin/dollar glyph for the Cost sidebar entry (Phase 14C). Outlined to
 * match the other nav icons; the dollar stroke renders in
 * `currentColor` so it picks up the active-state accent automatically.
 */
export function NavCostIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 6v12" />
      <path d="M15 9.2a3 3 0 0 0-2.5-1.4h-1a2.2 2.2 0 0 0 0 4.4h1a2.2 2.2 0 0 1 0 4.4h-1A3 3 0 0 1 9 15.2" />
    </svg>
  );
}

export function SaveIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M19 21H5V3h11l3 3z" />
      <path d="M8 21v-7h8v7M8 3v5h7" />
    </svg>
  );
}

export function PinIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M12 3v8M8 7h8M10 11l-3 8 5-3 5 3-3-8" />
    </svg>
  );
}

export function TagIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M20 13 11 22 2 13V4h9z" />
      <path d="M7 8h.01" />
    </svg>
  );
}

export function PlusIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}

/**
 * Outline shield used for Security surfaces (sidebar Health row,
 * Inventory shield badge, Quick Look Security section, Security view
 * row glyph). Severity colour is applied via the parent class
 * (`shield-badge.critical`, etc.) so the icon picks up `currentColor`.
 */
export function ShieldIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M12 3l8 4v5a9 9 0 0 1-8 9 9 9 0 0 1-8-9V7l8-4z" />
    </svg>
  );
}

/**
 * Filled check used for the Security view's "no findings" empty state.
 */
export function ShieldCheckIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" {...props}>
      <path d="M12 3l8 4v5a9 9 0 0 1-8 9 9 9 0 0 1-8-9V7l8-4z" />
      <path d="m9 12 2 2 4-4" />
    </svg>
  );
}
