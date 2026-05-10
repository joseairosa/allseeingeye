# COMPONENTS.md

Catalogue of every reusable React component in `apps/desktop/src/`.
Standing rule from the global `CLAUDE.md`: **check this file before
writing any new view or component**. If it exists here, render it with
the documented locals; never duplicate the markup.

When you add a new shared component or change one's public surface
(props, behaviour, observable side effects), update this file in the
same change. Stale entries are a bug.

---

## Table of contents

1. [How to use this file](#how-to-use-this-file)
2. [Layout shell](#layout-shell)
3. [Floating overlays](#floating-overlays)
4. [Reusable partials](#reusable-partials)
5. [Icons](#icons)
6. [Editor panes](#editor-panes)
7. [Cost view sub-panes](#cost-view-sub-panes)
8. [Onboarding steps](#onboarding-steps)
9. [Top-level views](#top-level-views)

---

## How to use this file

- **Before adding a `<button>`, `<input>`, `<dialog>`, or any new
  surface**, search this file for the pattern. If it exists, import
  and use it.
- **When you ship a new shared component**, add an entry below with
  file path, purpose, props, and a concrete usage example. The bar
  for "shared" is "rendered from more than one call site OR designed
  for that".
- **When you delete a shared component** (e.g. wave 3 audit cleanup),
  delete its entry here. A ghost catalogue entry is worse than none.
- **One-instance views** (InventoryView, HealthView, etc.) are
  catalogued for navigation but not for reuse - they read the global
  store and own their own data.

---

## Layout shell

These components compose the always-visible chrome. There is exactly
one instance of each per app; mounted from `App.tsx`.

### `TitleBar`

- **File**: `apps/desktop/src/components/TitleBar.tsx`
- **Purpose**: macOS-flavoured drag region + global theme toggle +
  density toggle. Renders the brand mark and reserves 80px on the
  left for native traffic lights.
- **Props**: none. Reads theme + density from `useUi`.
- **Usage**: rendered once in `App.tsx`, no external callers.

### `MainHeader`

- **File**: `apps/desktop/src/components/MainHeader.tsx`
- **Purpose**: per-view title + breadcrumb + the "Search or command
  ⌘K" launcher + the refresh-index icon button.
- **Props**: none. Reads `view` from `useUi`; fires
  `togglePalette(true)` on the search button and `startFullScan` on
  the refresh icon (with a busy state).
- **Usage**: rendered once in `App.tsx`.

### `Sidebar`

- **File**: `apps/desktop/src/components/Sidebar.tsx`
- **Purpose**: primary nav (Inventory / Map / Editor / Health / Cost /
  Security), per-tool filter shortcuts, per-type filter shortcuts,
  per-Health-pane focus shortcuts, footer with Onboarding tour and
  Settings entry.
- **Props**: none. Reads `view`, `tools`, `healthSummary`,
  `securitySummary` from `useUi` + IPC hooks. Sets `view`,
  `setSearch`, `setHealthFocus`, `toggleOnboarding` on click.
- **Internal subcomponents**:
  - `NavButton({ view, label, count?, alert?, icon })` — one nav row.
  - `ToolsGroup` / `TypesGroup` / `HealthGroup` — sections within the
    sidebar; not exported because they are tightly coupled to the
    parent's data flow.
- **Usage**: rendered once in `App.tsx`.

### `Statusbar`

- **File**: `apps/desktop/src/components/Statusbar.tsx`
- **Purpose**: footer line showing component count, scan time, watcher
  status, privacy mode.
- **Props**:
  | Name | Type | Default | Description |
  |---|---|---|---|
  | `resultCount` | `number` | required | Number of components currently in scope (post-filter). |
- **Usage**:
  ```tsx
  <Statusbar resultCount={components.length} />
  ```

---

## Floating overlays

Surfaces that mount over the main layout. Each owns its own open/close
state via `useUi`.

### `CommandPalette`

- **File**: `apps/desktop/src/components/CommandPalette.tsx`
- **Purpose**: ⌘K search-and-run palette. Fuzzy-searches components +
  navigates to actions (open settings, toggle theme, restart scan,
  etc.).
- **Props**:
  | Name | Type | Default | Description |
  |---|---|---|---|
  | `defaultQuery` | `string` | `""` | Pre-fill the input. Stories / tests only. Production callers pass nothing. |
- **Behaviour**: closes on backdrop click, Esc, or item activation.
  `defaultQuery` is consumed once on mount.
- **Usage**:
  ```tsx
  // App.tsx renders unconditionally; visibility is store-driven.
  <CommandPalette />
  ```

### `Onboarding`

- **File**: `apps/desktop/src/components/Onboarding.tsx`
- **Purpose**: 6-step first-launch flow (Welcome → Detect →
  Permission → Scan → Tour → Done). Steps live under
  `apps/desktop/src/components/onboarding/`.
- **Props**: none. Reads `onboardingOpen` from `useUi`.
- **Per-step components**: see [Onboarding steps](#onboarding-steps).
- **Usage**: rendered once in `App.tsx`.

### `QuickLook`

- **File**: `apps/desktop/src/components/QuickLook.tsx`
- **Purpose**: right-side slide-over with the selected component's
  detail (parsed body, security findings, "open in editor" CTA).
- **Props**: none. Reads `selectedComponentId`, `quickLookOpen` from
  `useUi`.
- **Internal subcomponents** (not exported):
  - `Header`, `Body`, `SecuritySection`, `SecurityRow`, `CostFooter`.
- **Usage**: rendered once in `App.tsx`.

### `DiagnosticsPanel`

- **File**: `apps/desktop/src/components/DiagnosticsPanel.tsx`
- **Purpose**: live event log + parse error list, used inside
  Settings → Diagnostics. Streams events via the pipeline-event
  channel.
- **Props**: none. Subscribes via `useDiagnosticsEvents`.
- **Usage**: rendered inside `SettingsView`'s `DiagnosticsPane`.

---

## Reusable partials

Small, highly reused UI building blocks.

### `RedactedPreview`

- **File**: `apps/desktop/src/components/RedactedPreview.tsx`
- **Purpose**: render a pre-redacted secret string (already redacted
  by Rust scanner) in a panic-mode-aware way. Intended for the
  Security view rows + QuickLook security section.
- **Props**:
  | Name | Type | Default | Description |
  |---|---|---|---|
  | `value` | `string` | required | Already-redacted text from the backend (`abcd…wxyz`). The full secret never reaches this component. |
  | `label` | `string` | `undefined` | Optional ARIA label for the masked element. |
  | `revealedFor` | `number` | `2000` | Auto-mask after this many ms. Set to `0` to disable; panic mode still wins. |
- **Usage**:
  ```tsx
  <RedactedPreview value={finding.redactedPreview} label="OpenAI key" />
  ```

### `SecretField`

- **File**: `apps/desktop/src/components/SecretField.tsx`
- **Purpose**: input-shaped masked field for editing a secret value
  (token, password, auth header). Click-to-reveal with auto-mask.
- **Props**:
  | Name | Type | Default | Description |
  |---|---|---|---|
  | `value` | `string` | required | Current secret value. |
  | `label` | `string` | `undefined` | ARIA label. |
  | `kind` | `"token" \| "password" \| "auth-header"` | `"token"` | Drives the icon glyph + placeholder text. |
  | `revealedFor` | `number` | `2000` | Auto-mask delay in ms. `0` disables (panic mode still wins). |
  | `disabled` | `boolean` | `false` | Disables the input + reveal toggle. |
  | `onCopyConfirmed` | `(value: string) => void` | `undefined` | Optional consumer-owned copy handler. The component never touches the system clipboard itself. |
- **Usage**:
  ```tsx
  <SecretField
    value={mcp.env.GITHUB_TOKEN ?? ""}
    label="GitHub PAT"
    kind="token"
    onCopyConfirmed={(v) => navigator.clipboard.writeText(v)}
  />
  ```

---

## Icons

All icons are SVG components in `apps/desktop/src/components/icons.tsx`
and accept the standard `SVGProps<SVGSVGElement>` (className, style,
aria-*). They render at 16px nominal size; size via CSS.

| Component | Use for |
|---|---|
| `TypeIcon({ id, className?, ...svg })` | Render a component-type glyph by `TypeIconId` (`icon-skill`, `icon-agent`, `icon-command`, `icon-mcp`, `icon-rule`, `icon-memory`, `icon-hook`, ...). Used in inventory rows + sidebar TypesGroup + map. |
| `CloseIcon` | "x" close button. |
| `SearchIcon` | Magnifying-glass. |
| `CommandSearchIcon` | Search + command-key composite. Used in MainHeader's palette launcher. |
| `RefreshIcon` | Refresh spiral. |
| `FiltersIcon` | Funnel. Used by inventory toolbar. |
| `DensityIcon` | Density toggle (rows). Used by titlebar. |
| `ThemeIcon` | Sun/moon. Used by titlebar. |
| `NavInventoryIcon` / `NavMapIcon` / `NavEditorIcon` / `NavHealthIcon` / `NavCostIcon` | Sidebar nav glyphs. |
| `SaveIcon` | Floppy. Used by editor toolbar. |
| `PlusIcon` | Plus. |
| `ShieldIcon` / `ShieldCheckIcon` | Security badges. ShieldCheck is the all-clear variant. |

Removed by audit cleanup (do not re-add until features ship):
- `PinIcon`, `TagIcon` — were used by QuickLook pin/tag buttons (audit
  #4 removed them).

Usage:
```tsx
<TypeIcon id="icon-skill" className="type-mini" aria-hidden="true" />
<NavCostIcon />
```

---

## Editor panes

The Editor view (`views/EditorView.tsx`) composes a form pane and a
raw-text pane side-by-side, with a shared edit reducer.

### `FormPane`

- **File**: `apps/desktop/src/views/editor/FormPane.tsx`
- **Purpose**: schema-driven input rendering. Walks the parsed AST,
  pairs each field with its JSON Schema entry, dispatches edits as
  pointer-keyed change events.
- **Props**:
  | Name | Type | Default | Description |
  |---|---|---|---|
  | `ast` | `FormAst` | required | The AST projected from raw text. |
  | `errors` | `readonly ValidationError[]` | required | Validator errors keyed by JSON pointer. |
  | `schema` | `SchemaShape \| null` | required | Parsed JSON Schema for the (tool, kind) tuple. `null` falls back to a minimal "edit raw text" stub. |
  | `format` | `string` | required | Format hint (markdownFrontmatter / json / etc.). Drives "additionalProperties" framing. |
  | `parseError` | `string \| null` | required | Last raw-buffer parse error. Banner above the fields when non-null. |
  | `onFieldChange` | `(pointer: string, value: unknown) => void` | required | Called on every field change; pointer is the JSON pointer (`"/name"`, `"/env/GITHUB_TOKEN"`). |
  | `empty` | `boolean` | `false` | True when no component is selected; renders an empty state. |
- **Helpers exported alongside**:
  - `parseSchema(text: string | null): SchemaShape | null`
- **Usage**:
  ```tsx
  <FormPane
    ast={state.formAst}
    errors={state.validation?.errors ?? []}
    schema={schema}
    format={format}
    parseError={state.parseError}
    onFieldChange={handleFieldChange}
  />
  ```

### `MonacoRawPane`

- **File**: `apps/desktop/src/views/editor/MonacoRawPane.tsx`
- **Purpose**: lazy-loaded Monaco editor for the raw-text pane.
- **Props**:
  | Name | Type | Default | Description |
  |---|---|---|---|
  | `content` | `string` | required | Buffer text. Source of truth. |
  | `format` | `Format` | required | Drives Monaco language id (json, yaml, ini=toml, markdown, plaintext for binary). |
  | `onChange` | `(next: string) => void` | `undefined` | Phase 3.3 wiring; called on every keystroke. |
- **Usage**:
  ```tsx
  <Suspense fallback={<MonacoSkeleton />}>
    <MonacoRawPane content={state.currentRaw} format={format} onChange={handleRawChange} />
  </Suspense>
  ```

---

## Cost view sub-panes

The Cost view (`views/CostView.tsx`) composes four sub-panes. None of
them touch IPC directly; they accept already-fetched data + an
`isLoading` flag so the parent stays in charge of caching.

### `CostKpiStrip`

- **File**: `apps/desktop/src/views/cost/CostKpiStrip.tsx`
- **Props**:
  | Name | Type | Default | Description |
  |---|---|---|---|
  | `summary` | `SummaryResponse \| undefined` | required | The summary payload; undefined means pre-first-fetch. |
  | `isLoading` | `boolean` | required | Drives the skeleton cards. |
- **Usage**:
  ```tsx
  <CostKpiStrip summary={summary.data} isLoading={summary.isPending} />
  ```

### `CostByProject`

- **File**: `apps/desktop/src/views/cost/CostByProject.tsx`
- **Props**:
  | Name | Type | Default | Description |
  |---|---|---|---|
  | `rows` | `ReadonlyArray<ByProjectRow>` | required | Sorted-by-cost rows. |
  | `isLoading` | `boolean` | required | Skeleton trigger. |
  | `limit` | `number` | `10` | Cap shown; excess rolls into a footer counter. |
- **Usage**:
  ```tsx
  <CostByProject rows={byProject.data ?? []} isLoading={byProject.isPending} limit={10} />
  ```

### `CostByDay`

- **File**: `apps/desktop/src/views/cost/CostByDay.tsx`
- **Purpose**: 30-day SVG sparkline, no chart library, peak callout.
- **Props**:
  | Name | Type | Default | Description |
  |---|---|---|---|
  | `rows` | `ReadonlyArray<ByDayRow>` | required | Day rows in `day ASC`. |
  | `isLoading` | `boolean` | required | |
- **Usage**:
  ```tsx
  <CostByDay rows={byDay.data ?? []} isLoading={byDay.isPending} />
  ```

### `CostRecommendations`

- **File**: `apps/desktop/src/views/cost/CostRecommendations.tsx`
- **Props**:
  | Name | Type | Default | Description |
  |---|---|---|---|
  | `recs` | `ReadonlyArray<CostRec>` | required | Recommendation cards. |
  | `isLoading` | `boolean` | required | |
  | `limit` | `number` | `5` | Spec cap; never render more than this. |
- **Usage**:
  ```tsx
  <CostRecommendations recs={recs.data ?? []} isLoading={recs.isPending} />
  ```

---

## Onboarding steps

Each onboarding step is a separate component composed by `Onboarding`.
They share a generic `OnboardingStepProps` shape (`{ state, actions,
tools }` with strongly-typed `OnboardingState` + `OnboardingActions`).

| Component | File | Step name | Purpose |
|---|---|---|---|
| `Welcome` | `components/onboarding/Welcome.tsx` | 1 | Brand intro + "Get started". |
| `Detect` | `components/onboarding/Detect.tsx` | 2 | Auto-detect installed tools (Claude Code, Codex, Cursor, Antigravity). Lists what was found, lets user toggle inclusion. |
| `Permission` | `components/onboarding/Permission.tsx` | 3 | Path-readability check + explainer about local-only privacy. |
| `Scan` | `components/onboarding/Scan.tsx` | 4 | Run the initial full scan, show progress + counts. |
| `Tour` | `components/onboarding/Tour.tsx` | 5 | 4-frame "what to expect" tour. |
| `Done` | `components/onboarding/Done.tsx` | 6 | "Ready" handoff. Closes onboarding on click. |

Shared types in `components/onboarding/types.ts`. Update both that
file AND this row when changing the step contract.

---

## Top-level views

One per `ViewId` in the store. Mounted in `App.tsx`; visible iff
`useUi.view` matches. Reads its data via TanStack Query hooks; owns
no externally-rendered API.

| Component | File | View id | Reads |
|---|---|---|---|
| `InventoryView` | `views/InventoryView.tsx` | `inventory` | `useComponents`, `useComponentFindingsCounts` (via search-string filter parser) |
| `MapView` | `views/MapView.tsx` | `map` | `useTools` (decorative stub graph; real Sigma.js graph in v1) |
| `EditorView` | `views/EditorView.tsx` | `editor` | `useComponentWithRaw`, `useValidationSchema`, `useSaveComponent` |
| `HealthView` | `views/HealthView.tsx` | `health` | `useComponents({ kind: "mcp" })`, `useComponents({ kind: "memory" })`, `useHealthSummary` |
| `SecurityView` | `views/SecurityView.tsx` | `security` | `useSecuritySummary`, `useSecurityFindings`, `useSuppressFinding`, `useUnsuppressFinding` |
| `CostView` | `views/CostView.tsx` | `cost` | `useCostSummary`, `useCostByProject`, `useCostByDay`, `useCostRecommendations`, `useCostRefresh` |
| `SettingsView` | `views/SettingsView.tsx` | `settings` | composes 8 panes (General, Tools, Index, Health, Backup, Privacy, Updates, Diagnostics) |

### Settings panes

Internal to `SettingsView.tsx`; not exported. Listed for navigation:

- **GeneralPane** — theme / density / reduced-motion.
- **ToolsPane** — per-tool indexed toggle (audit issue #2).
- **IndexPane** — DB path, project memory roots, rebuild, reset.
- **HealthPane** — MCP probing default + bloated-memory threshold.
- **BackupPane** — Phase 15: status, "Backup now", "Preview restore",
  "Restore now…", auto-backup toggle.
- **PrivacyPane** — telemetry status pill + diagnostics export.
- **UpdatesPane** — channel, auto-check, "Check now".
- **DiagnosticsPane** — wraps the `DiagnosticsPanel` component.

To add a new settings pane, add the function in `SettingsView.tsx`
and append `<NewPane />` to the `settings-layout` grid in
`SettingsView`. Do not extract to a separate file unless it grows
beyond ~150 lines.

---

## Removed by audit (do not re-add without feature)

These components or controls were removed by wave 3 of the button
audit because they advertised functionality the app does not deliver.
Re-adding without the underlying feature reintroduces the same audit
finding.

| Removed | Reason | Audit issue |
|---|---|---|
| QuickLook pin button | Pin system not in data model | #4 |
| QuickLook tag button | Tag system not in data model | #4 |
| Map graph node interactivity | No selection model in stub graph | #12 |
| Settings dyslexia-friendly font row | Font asset not bundled | #13 |
| Map cluster-mode picker | No clusterMode store field | #14 |
| Sidebar "Add tool" row | Registry is hardcoded; no runtime tool registration | #16 |
| Health view "probe selected" toolbar button | MCP probing pipeline not built | #1 |
