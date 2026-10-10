Bugra is a dark-first, single-accent system for desktop apps, websites and, later, mobile: warm black surfaces, one green accent (Android green), Geist for text and JetBrains Mono for code, generous radii, hairline borders instead of shadows. It reads as calm, technical and personal. Light is a derived counterpart theme.

## Content fundamentals

- Write short, plain sentences in sentence case, in active voice. Address the reader as "you". No exclamation marks, superlatives, filler or emoji.
- Guides and docs are task-first: a step says what to do, then one line on why. Commands go in a `CodeBlock` with a copy button, never inline in prose.
- A button names the action in one or two words, verb first: "Copy prompt", "Save changes". Keep the name through the flow: "Publish" leads to "Published".
- Notes and dialogs title the rule or decision ("Back up first", "Delete 8 files?"); the body says why, in muted text.
- Errors say what went wrong and how to fix it, start with the word "Error:", and never apologize. Empty states point to the next action.
- Name things by what the user understands, not by how the system is built.
- Output stays ASCII-safe: prefer "-" and "..." over typographic dashes and ellipsis characters in generated text, logs and filenames.
- Write for many languages from the start: keep strings out of markup, never build sentences from fragments, and let text grow (German and Russian run 20-30% longer than English).

## Visual foundations

- Ground: page on `bg`, raised areas on `surface`, hover fills, pills and switch tracks on `surface-2`. Text is `ink`; secondary text, nav links and captions are `muted`. Code and prompt blocks sit on `code-bg` with `code-muted` labels.
- One accent: `accent` (Android green) is the only chromatic brand color. Use it as a fill for the primary action, copied state, step badges and switch tracks, always with `on-accent` text; use `accent-ink` when the green is text, a link or an outline. `ok`, `warn` and `info` are status text only and always carry a word or icon; blue `info` keeps notices apart from green `ok` and coral `warn`.
- Themes: dark is the default; follow the system setting when the platform offers one. Light swaps every ground and ink, keeps the accent fill, and uses `accent-ink` for outlines so edges reach 3:1. Consume tokens; never hard-code hex.
- Type: `ui` (Geist) for everything except code, with a system-sans fallback; `mono` (JetBrains Mono) for code, prompts and footer meta. Geist is loaded at 400, 500 and 600 only, so headings are 600 with tight tracking (`display` -0.03em, `h2` -0.02em), balanced wraps and pretty paragraphs. Body line-height 1.65, code 1.7. Prose at most 66ch; page column 900px.
- Type as design: let size, weight and tracking carry the headline. Do not accent one word in italic, bold or color, do not set labels in all caps, and add no small labels above content that does not need them. The one exception is the social image, where the subject phrase is set in `accent`.
- Fluid sizes: `display` 36-60px and `h2` 26-36px with a viewport clamp; gutter 20-64px; section padding 44-72px; hero top 56-104px. The `space-*` tokens are the stops.
- Shape: boxes `radius` (14px), notes, code blocks, menus, fields and snackbars `radius-sm` (10px), menu rows `radius-xs` (7px), every button, chip, toggle and badge a `pill`.
- Borders, not shadows: divide sections with a 1px `line` top border; outline pills, code blocks and menus with `line-strong`. `line-strong` is faint (about 1.7:1), so it never marks an input or switch: those use `border-control` (3:1 or better). Only floating layers (menus, snackbars, dialogs) get a shadow.
- Structure: a sticky masthead (blurred `bg` at 88%, `line` bottom border); sections stacked with hairlines; steps as a numbered list only because the content is a real sequence.
- Texture: an optional fixed film-grain overlay at 3.5% opacity, non-interactive. The social image has one soft green glow; no other gradients.
- State layers: hover 8%, focus 10%, pressed 10%, dragged 16%; disabled content 38% and container 12% (`state-*`, `content-disabled`, `container-disabled`).
- Focus and selection: a solid 2px `focus-ring` outline, 3px offset, on every interactive element; selected text uses `selection`.
- Targets: at least 48px (`target-min`) for anything tappable on touch; on a pointer-only desktop surface dense controls may use 34px (`control-sm`), and every control stays reachable by keyboard.
- Motion: 100-500ms on `ease-out` (source default); use `ease-standard` for utility motion, `ease-enter` for elements arriving and `ease-exit` for leaving. Smooth-scroll anchors. Under `prefers-reduced-motion` cut all transitions to near zero. One collapsing banner at most; no entrance animations on every section.
- Restraint: one accent, one hero headline, everything else quiet. Build for phone width first.

## Iconography

- No icon set or logo file was supplied. The wordmark is the name in `wordmark` type, optionally with a small outline monitor glyph in `accent-ink` (a placeholder to replace).
- UI icons are single-stroke outline SVGs on a 24px grid, 1.6px stroke, round caps and joins, drawn in `muted` and turning to `ink` on hover; sizes 15px (globe), 16px (promo, close), 18px (social), 19px (nav). The `assets/Icons` group holds the inline set (globe, chevron-down, close, check, mail), each drawn in one ink named in the group's README.
- Brand marks of other companies (X, GitHub, LinkedIn) are not bundled; use each company's own official asset.
- Never use emoji or filled multi-color icons.

## Components

Twenty-one components, all in `Bugra` (`window.Bugra`): SkipLink, Wordmark, Masthead, NavLink, IconButton, LanguageMenu, Button, CopyButton, PromoBar, Hero, Section, Steps, CodeBlock, Note, Footer and SocialCard come from the source site. **Intentional additions** that close gaps for apps and forms: Chip, TextField, Switch, Dialog and Snackbar, each labeled "(added)" in its guide. Read a component's card before using it.

## Further sections

- `desktop-and-mobile.md`: desktop app and window guidance, mobile and Android mapping to Material 3 (state layers, targets, motion, safe areas).
- `layout-and-languages.md`: page structure, breakpoints, right-to-left and CJK handling, social image sizes.
- `accessibility.md`: contrast table, focus, targets, motion, status colors.
