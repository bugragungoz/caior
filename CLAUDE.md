# CAIOR (croxz all-in-one runtimes) - working notes

Read `docs/PLAN.md` first; it holds the decisions, architecture and phase list. Package details: `docs/PACKAGES.md`, catalog schema: `docs/CATALOG-FORMAT.md`, UI: `docs/DESIGN.md`.

## Rules

- Never commit installer binaries, bundles or build output. Downloads go to `cache/` or `_local/` (both gitignored).
- `legacy/` is reference only (v1 PowerShell tray installer, v2 Tauri app). Do not build on it or edit it.
- `_local/` holds the old 943 MB v1 build and its `runtimes/` folder; local only, do not delete without asking.
- Colors, type, spacing and radii come from `design/bugra/tokens.json`; `crates/app/build.rs` generates `theme.slint` into OUT_DIR on every build. No hand-written hex or sizes in `ui/*.slint`.
- UI text: English source strings in `@tr(...)`, Turkish in `i18n/tr/LC_MESSAGES/caior.po` (add every new string there). Rust only passes numbers, names and states.
- Look at the UI with `CAIOR_SNAPSHOT=<file.png> [CAIOR_PAGE=n] [CAIOR_LANG=tr] caior.exe`; test GUI installs in Sandbox with `tools/sandbox/start.ps1 -Gui`.
- No subagents or workflows: the user's session limit is small.
- UI copy follows the Bugra content rules: short sentences, sentence case, no exclamation marks or emoji, verb-first buttons, errors start with "Hata:" / "Error:". Every string goes through `@tr` (TR + EN).
- Every catalog entry needs a verified official URL, silent args checked against vendor docs, a detection rule, and either a SHA256 or a signer allowlist.
- Docs are in Turkish; code, identifiers and comments are in English.
- Test real installs only in Windows Sandbox or a VM, never on the host without asking.
- Remote: github.com/bugragungoz/caior (public). Push and tag only when the user asks.
- The user switches effort before coding phases; announce before starting a new phase.
