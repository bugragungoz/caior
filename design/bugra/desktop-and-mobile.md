# Desktop and mobile

Bugra targets laptop and desktop apps and websites first, and phones after that. This page says how the system changes across them. Android green (`accent`) is the brand accent; where the source had no rule, Material 3 conventions fill the gap.

## Sources and confidence

- Verified from the Material Components for Android docs: the motion easing and duration tokens and the shape scale (4, 8, 12, 16, 20, 28, 32, 48dp, full).
- The Material 3 state layer opacities (hover 8%, focus 10%, pressed 10%, dragged 16%; disabled content 38%, container 12%) and the 48dp touch target are the published Material 3 and Android accessibility values, entered from the spec rather than re-read live.

## Desktop apps and websites

- Default to the dark theme and follow the system theme setting (`prefers-color-scheme`) when it is set; the light theme is derived, so review it in your own screens.
- Pages: one 900px column (`measure`) with a fluid gutter. Apps: fill the window; use `surface` panes on `bg` divided by `line` rules, never heavy chrome.
- Pointer surfaces have hover: give every control a hover fill (`surface-2`) and an `ink` border on outline buttons, plus a visible focus ring for keyboard use. Nothing may be reachable only by hover.
- Keyboard first: logical tab order, a skip link on pages, Escape closes menus and dialogs, and focus returns to the trigger.
- Dense desktop toolbars may use `control-sm` (34px) rows; anything a finger might hit stays at `target-min` (48px).
- Long-running work shows progress in place; results and errors appear next to the control that caused them, and a `Snackbar` confirms actions with no lasting state.
- Scrollable areas (code, logs) are focusable and keep the `code` type.

## Mobile web and Android apps

- Minimum target 48dp (`target-min`). The masthead icon buttons and nav links are already padded up to it.
- Respect safe areas: pad fixed bars with `env(safe-area-inset-*)` on the web and system insets on Android.
- Window size classes: compact under 600dp, medium 600-839dp, expanded 840dp and up. On compact widths keep one column, collapse secondary text, and let the language menu show only its globe.
- No hover on touch: pressed state is the feedback (`state-pressed`), and information is never hidden behind hover.
- Follow the system dark and light setting; keep the accent fill the same in both.

## Color roles (Material 3)

| Material 3 role | Bugra token |
| --- | --- |
| background, surface | `bg` |
| surfaceContainer | `surface` |
| surfaceContainerHigh | `surface-2` |
| onSurface | `ink` |
| onSurfaceVariant | `muted` |
| outlineVariant | `line` |
| outline | `border-control` |
| primary | `accent` (fill); `accent-ink` where primary is text or an outline |
| onPrimary | `on-accent` |
| primaryContainer | `accent-quiet` |
| error | `warn` |
| scrim | `scrim` |

## Shape

Bugra's radii are a custom scale. In Compose or MaterialShapes set: extra small 4dp (`radius-2xs`), small 10dp (`radius-sm`), medium and large 14dp (`radius`), extra large and above 14dp too, and full for pills (`pill`). Do not use Material's 28dp extra-large corners on cards or dialogs.

## Type

Geist is not an Android system font: bundle it (or use a downloadable Google font) and fall back to the platform sans-serif (Roboto). Approximate mapping from Material roles: displayLarge -> `display`; headlineLarge -> `h2`; titleMedium -> `h3`; bodyLarge -> `body`; bodyMedium and bodySmall -> `small`; labelLarge -> `button`. Sizes are dp on Android.

## State

State layers draw a layer of the control's own content color at the `state-*` opacities over its container. Disabled: content at 38%, container at 12%. Ripple color is `ink` at `state-pressed`.

## Motion

| Use | Duration | Easing |
| --- | --- | --- |
| Hover, toggles, chips | `dur-hover` 200ms | `ease-out` |
| Small utility transitions | `dur-instant` 100ms to `dur-close` 250ms | `ease-standard` |
| Dialogs and snackbars arriving | `dur-medium` 300ms | `ease-enter` |
| Anything leaving | 200ms | `ease-exit` |
| Banner expanding | `dur-expand` 500ms | `ease-out` |

Duration grows with the distance an element travels. Honor the system reduce-motion setting.
