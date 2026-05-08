# All Seeing Eye design prototype

Static app prototype for the future Tauri 2 desktop app.

## Files

- `index.html` - desktop shell prototype with Inventory, Map, Editor, Health, Quick Look, command palette, and onboarding states.
- `styles.css` - design tokens, dark/light themes, density modes, responsive behavior, and component styling.
- `app.js` - small static interactions for navigation, filtering, theme/density toggles, Quick Look, and Cmd-K.
- `assets/eye-logo.svg`, `assets/eye-mark.svg`, and `assets/logo-lockup.svg` - identity assets.
- `assets/logo-options-board.png` - generated logo exploration board; option C is the selected direction.
- `assets/type-icons.svg` - reusable SVG symbols for component type icons.
- `tokens.json` - portable design-token values for the app implementation.

## Notes for implementation

- MVP should prioritize Inventory, Quick Look, Editor, onboarding, diagnostics, and Cmd-K, matching `docs/10-roadmap.md`.
- Map, drift, and MCP health are included here as visual direction even though they are v1 scope.
- CSS variables mirror the color and spacing tokens in `docs/07-visual-design.md`.
- The static data is intentionally representative, not exhaustive.
