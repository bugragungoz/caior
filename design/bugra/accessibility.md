# Accessibility

## Contrast (measured, WCAG 2)

Text on `bg`, `surface` and `surface-2` in both themes meets 4.5:1 for `ink`, `muted`, `accent-ink`, `ok`, `warn` and `info`; `ink` reaches 14:1 or better. `border-control` reaches 3:1 or better on all three grounds in both themes. `on-accent` on `accent` is 10.5:1. `code-muted` on `code-bg` is 4.9:1 or better.

Known gaps, kept because they are the source's values: `line-strong` is about 1.7:1 on `bg`, so it is decorative and never the only edge of a control; `partner-cyan` is an icon stroke beside a text label only; the light-theme `accent` fill is about 1.6:1 against `bg`, so every accent-filled control also has an `accent-ink` outline or a text label at full contrast.

## Rules

- Keyboard focus is a solid 2px `focus-ring` outline, 3px offset, on every interactive element; never remove it.
- Touch and pointer targets are at least 48px (`target-min`).
- Status never relies on color alone: `ok`, `warn` and `info` always come with a word or icon; selected chips get a check; errors start with the word "Error:". `ok` (green), `warn` (coral) and `info` (blue) differ enough in lightness and hue for red-green color blindness, but the word is what carries the meaning.
- Respect `prefers-reduced-motion`: transitions drop to near zero and the promo bar appears without animation.
- Provide a skip link, use landmarks (`header`, `nav`, `main`, `footer`), label every icon button, and give every input a visible label.
- Set `lang` and `dir` on the page and `lang` on each language row.
- Code blocks are focusable (`tabindex=0`) so keyboard users can scroll them.
