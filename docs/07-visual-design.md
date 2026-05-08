# 07 - Visual Design

The look. Goal: dense but airy, dark-first, glassy, motion-rich without being distracting. The product should feel like a piece of high-end pro audio software (Ableton, Logic) crossed with an observatory: precise, calm, data-forward, vaguely magical.

## Mood board, in words

- **Datadog night dashboards** for density and information legibility.
- **Linear** for precision typography and motion discipline.
- **Arc Browser** for the playful but restrained chrome.
- **Synthwave / sci-fi UIs** for accent gradients and the "all seeing eye" identity.
- **Apple Vision OS** for layered glass and depth, used sparingly.

We are explicitly **not** going for:
- Cartoony illustrations or mascots.
- Neumorphism.
- Heavy skeuomorphism.
- Pastel marketing gradients on every surface.

## Identity

### Logo

Concentric eye:

```
            . . . . . .
        .                 .
      .       _____         .
     .       /     \         .
    .       |   *   |         .
    .        \_____/          .
     .                         .
      .                       .
        .                  .
            . . . . . . .
```

A circle of dots (the iris), a soft inner ring (the cornea), a central pupil with a single accent star/diamond. Geometric and symmetrical. Renders crisp at 16x16 and 1024x1024.

Colour treatment:
- Light surfaces: deep iris (`#101A2E`), pupil accent (electric blue `#3B82F6`), star (cyan `#22D3EE`).
- Dark surfaces: pale iris (`#E5E7EB`), pupil accent (electric blue `#60A5FA`), star (cyan `#67E8F9`).

### Wordmark

`All Seeing Eye` in a precise grotesque - choices in priority: `Geist`, `Inter Tight`, `Manrope`. All-lowercase optional in app chrome; sentence case in marketing.

## Typography

| Role | Font | Weight | Notes |
|------|------|--------|-------|
| UI primary | Inter Tight (Geist as alt) | 400 / 500 / 600 | Variable; ligatures on |
| Mono / code / paths | JetBrains Mono (or Geist Mono) | 400 / 500 | Used for paths, raw editor, IDs |
| Numerics | Inter tabular | 500 | Tabular for stat panels |

Type scale (rem at 16px base):

| Token | Size | Line | Use |
|-------|------|------|-----|
| `t-xs` | 0.75 | 1.0 | meta, breadcrumbs |
| `t-sm` | 0.825 | 1.25 | secondary labels |
| `t-base` | 0.9375 | 1.4 | body |
| `t-md` | 1.0 | 1.5 | comfortable body |
| `t-lg` | 1.125 | 1.4 | section headers |
| `t-xl` | 1.5 | 1.25 | view headers |
| `t-2xl` | 2.0 | 1.15 | hero / empty state |
| `t-3xl` | 2.5 | 1.1 | onboarding |

Letter spacing tightens slightly above `t-xl` (-0.5%).

## Colour

Two themes - dark first, light parity.

### Dark theme (default)

| Token | Hex | Use |
|-------|-----|-----|
| `bg-base` | `#0B0E14` | App body |
| `bg-elev-1` | `#11151D` | Side panels |
| `bg-elev-2` | `#161B25` | Cards, rows |
| `bg-elev-3` | `#1D2330` | Hover, modals |
| `bg-glass` | `rgba(20,25,35,0.6)` | Floating panels w/ backdrop blur |
| `border-subtle` | `#202635` | Default borders |
| `border-strong` | `#2C3447` | Selected, focused |
| `text-primary` | `#E5E7EB` | Body |
| `text-secondary` | `#9CA3AF` | Meta, helper |
| `text-tertiary` | `#6B7280` | Disabled |
| `accent-1` | `#60A5FA` | Primary action, links |
| `accent-2` | `#67E8F9` | Highlight, sparks |
| `accent-3` | `#A78BFA` | Map edge, secondary |
| `success` | `#10B981` | Up, healthy |
| `warn` | `#F59E0B` | Degraded |
| `error` | `#EF4444` | Down, parse error |
| `cold` | `#64748B` | Stale |

### Light theme

| Token | Hex |
|-------|-----|
| `bg-base` | `#F7F8FA` |
| `bg-elev-1` | `#FFFFFF` |
| `bg-elev-2` | `#F1F3F7` |
| `bg-elev-3` | `#E8EBF0` |
| `bg-glass` | `rgba(255,255,255,0.7)` |
| `border-subtle` | `#E5E7EB` |
| `border-strong` | `#CBD5E1` |
| `text-primary` | `#0F172A` |
| `text-secondary` | `#475569` |
| `text-tertiary` | `#94A3B8` |
| `accent-1` | `#2563EB` |
| `accent-2` | `#0EA5E9` |
| `accent-3` | `#7C3AED` |

Both themes pass WCAG AA on body text.

## Spacing and grid

8-point grid. Tokens `s-1` (4px), `s-2` (8), `s-3` (12), `s-4` (16), `s-5` (24), `s-6` (32), `s-7` (48), `s-8` (64).

Side bar 240px. Editor max content 920px wide for body Markdown; raw view fluid. Inventory rows fixed at 56px tall on default density; user-toggleable to compact 40px.

## Surfaces and elevation

Three elevation layers, plus glass.

```
elev 0 = bg-base (no shadow)
elev 1 = bg-elev-1 + 1px border-subtle, shadow none
elev 2 = bg-elev-2 + 1px border-subtle, shadow soft
elev 3 = bg-elev-3 + 1px border-strong, shadow medium
glass  = bg-glass + 1px border-strong + backdrop-filter blur(24px) saturate(180%)
```

Glass is reserved for floating chrome: command palette, slide-in Quick Look, modals. Never used for the main view.

## Iconography

- 24px primary icon size in chrome; 16px in dense rows.
- Stroke 1.5px, geometric, lined - **Lucide** family.
- Type icons (skill / agent / command / mcp / hook / rule / memory / plugin) are custom - tiny, monogrammatic, unique to All Seeing Eye. Mock examples:

```
skill    o      (a single dot inside a ring)
agent    ⬡      (hex)
command  ▸      (right-pointing wedge)
mcp      ◇      (diamond)
hook     △       (triangle)
rule     □      (square)
memory   ▦      (hatched square)
plugin   ⊕      (cross-circle)
session  ≣      (three lines)
```

These shapes are reused in the Map view as node glyphs.

## Motion

Disciplined. No bouncy spring. Two easings:

- `ease-out-quart` for entries (panels, popovers, toasts) - fast in, settled.
- `ease-in-out-cubic` for layout shifts - graceful.

Durations:
- Tooltip / hover state: 80 ms.
- Panel slide-in: 240 ms.
- View transition: 360 ms.
- Map node selection: 200 ms with a single radial pulse.

`prefers-reduced-motion` flips all of the above to instant transitions and replaces pulses with simple opacity swaps.

## Sound (optional, off by default)

Two sounds, tasteful, < 200 ms:
- `tick` on save success.
- `pluck` on Quick Look open.

Off by default. User opts in.

## Layout language

A clear hierarchy of containers:

```
window
+-- titleBar (drag region; window controls)
+-- sidebar
+-- main
    +-- header (breadcrumb + toolbar + search)
    +-- content (the active view)
    +-- footer (status / counts / quick actions)
+-- right panel (slide-in; not always present)
```

The header bar is always 48px tall. The footer is 32px. The titleBar is `transparent` on macOS, with the traffic-light controls inset into the window chrome (no large white bar).

## Density modes

Two: **comfortable** (default) and **compact**. Both ship from day one.

- Comfortable: 56px row height, 16px paddings, 14.5px body.
- Compact: 40px row height, 12px paddings, 13px body.

Toggle in settings or via Cmd-Shift-D.

## Visualisations

The Map view is the most visually expressive surface.

### Node design

```
       +---------+
       |   o     |  <- glyph indicates type
       |   spec  |  <- name, truncated
       |  Claude |  <- tool, dim
       +---------+
```

Edge stroke 1px; styled by relation kind:
- `bundles` - solid `accent-3`
- `imports` - dashed `text-secondary`
- `equivalentTo` - double-line `accent-2`
- `dependsOn` - solid `text-secondary`

Nodes glow softly when selected; selected neighbours get a 30% glow.

### Health palette

Health uses status colours consistently:

```
up         success green
degraded   warn amber
down       error red
unprobed   text-tertiary grey
cold       cold blue-grey
```

Always paired with an icon: up uses a small dot; degraded a half-filled dot; down an open ring with a bar; unprobed a dotted ring.

## Empty and zero states

Empty states get full hero-quality artwork - a monumental rendering of the eye logo with very light constellation lines, copy under it. Single primary action, one secondary.

```
                 .  .  .
              .           .
            .   ___   ___   .
           .   /   \ /   \   .
          .   |  *   *  |    .
           .   \___/ \___/   .
            .               .
              .           .
                 .  .  .

         No agentic tools detected.

      [ Detect tools ]      [ Pick manually ]
```

## Surfaces in dark and light

Two reference compositions, in words.

- **Dark Inventory** - body `bg-base` charcoal blue; sidebar one shade lighter; rows another shade lighter on hover. Accent blue glows quietly on selected row's left border (3px). Type icon in `text-secondary` with the type initial in a 16x16 rounded glyph at 80% opacity.

- **Light Editor** - body off-white, header crisp white with a 1px hairline border. Frontmatter form on a slightly grey card. Raw view with code-style syntax highlighting using a low-saturation palette so prose reads first, code second.

## Brand voice (microcopy)

Confident, technical, terse. Lowercase preferred in chrome. No emoji. No exclamation marks except in errors. No `!`-prefixed status; we use icons.

Good examples:
- "237 components"
- "no on-disk changes"
- "merge required - 3 pairs diverged"

Avoid:
- "Awesome! You've connected your tool!"
- "Whoops, something went wrong"
- "Don't worry, your data is safe"

## Theming and customisation

MVP ships dark and light. v1 adds:
- High-contrast variants of both.
- Custom accent picker (single accent token; everything else derives).
- Editor colour scheme (separate from app theme; Monaco themes apply).

## Accessibility recap (visual)

- AA contrast on all body text and meta.
- Focus ring is a 2px outer ring in `accent-1` with 2px offset; never removed.
- Colour is never sole signifier; icons + text accompany.
- Reduced motion swaps animations for instant transitions.
- Dyslexia-friendly font option in settings.

## Asset list (for design hand-off)

- Logo: SVG mono, SVG full, ICO 16/32/256, ICNS, dock icon 1024.
- Type icons: skill / agent / command / mcp / hook / rule / memory / plugin / session / task / output-style / statusline / permission / marketplace / tool. SVG, 16 and 24.
- Empty state hero illustrations (5).
- Onboarding artwork (3).
- Marketing screenshots (Inventory, Map, Editor, Health) at 1440x900.

## Components library

Built on **Radix UI Primitives** for behaviour, **Tailwind v4** for styling, **CVA** for variants. Custom components in priority order:

- Sidebar
- Inventory row
- Component card
- Quick Look panel
- Diff view (drift)
- Map (Sigma.js or custom WebGL with `pixi-react`)
- Form-from-schema renderer (drives Editor's left pane)
- Schema-aware Markdown editor (Monaco + custom YAML frontmatter island)
- Toast
- Command palette (`cmdk`)
- Status badge

Every custom component has a Storybook entry. Storybook is part of the repo from week one.
