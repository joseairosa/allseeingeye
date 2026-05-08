import { useEffect, useRef, useState } from "react";
import { useUi, type ViewId } from "@/store/ui";
import { SearchIcon } from "./icons";

interface PaletteItem {
  id: string;
  kind: string;
  kindClass?: string;
  label: string;
  meta: string;
  jumpTo?: ViewId;
}

const ITEMS: PaletteItem[] = [
  { id: "spec", kind: "skill", label: "spec", meta: "Claude Code", jumpTo: "editor" },
  { id: "spec-verify", kind: "skill", label: "spec-verify", meta: "Claude Code" },
  { id: "spec-cmd", kind: "command", label: "/spec", meta: "Claude Code" },
  { id: "find-drift", kind: "action", kindClass: "action", label: "Find memory drift", meta: "Health", jumpTo: "health" },
];

export function CommandPalette() {
  const open = useUi((s) => s.paletteOpen);
  const toggle = useUi((s) => s.togglePalette);
  const setView = useUi((s) => s.setView);
  const inputRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("spec");

  useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);

  const visible = ITEMS.filter((it) =>
    it.label.toLowerCase().includes(query.toLowerCase()),
  );

  return (
    <div
      className={`palette-backdrop${open ? " open" : ""}`}
      aria-hidden={!open}
      onClick={(e) => {
        if (e.target === e.currentTarget) toggle(false);
      }}
    >
      <div className="command-palette" role="dialog" aria-modal="true" aria-label="command palette">
        <label className="palette-search">
          <SearchIcon />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            aria-label="command search"
            placeholder="search..."
          />
        </label>
        <div className="palette-results">
          {visible.map((it, idx) => (
            <button
              key={it.id}
              type="button"
              className={`palette-row${idx === 0 ? " active" : ""}`}
              onClick={() => {
                if (it.jumpTo) setView(it.jumpTo);
                toggle(false);
              }}
            >
              <span className={`palette-kind${it.kindClass ? ` ${it.kindClass}` : ""}`}>
                {it.kind}
              </span>
              <strong>{it.label}</strong>
              <span>{it.meta}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
