# CAIOR (croxz all-in-one runtimes) - working notes

Read `docs/PLAN.md` first; it holds the decisions, architecture and phase list. Package details: `docs/PACKAGES.md`, catalog schema: `docs/CATALOG-FORMAT.md`, UI: `docs/DESIGN.md`.

## Rules

- Never commit installer binaries, bundles or build output. Downloads go to `cache/` or `_local/` (both gitignored).
- `legacy/` is reference only (v1 PowerShell tray installer, v2 Tauri app). Do not build on it or edit it.
- `_local/` holds the old 943 MB v1 build and its `runtimes/` folder; local only, do not delete without asking.
- Colors, type, spacing and radii come from `design/bugra/tokens.json` through the generated `ui/theme.slint`. No hand-written hex in `.slint` files.
- UI copy follows the Bugra content rules: short sentences, sentence case, no exclamation marks or emoji, verb-first buttons, errors start with "Hata:" / "Error:". Every string goes through `@tr` (TR + EN).
- Every catalog entry needs a verified official URL, silent args checked against vendor docs, a detection rule, and either a SHA256 or a signer allowlist.
- Docs are in Turkish; code, identifiers and comments are in English.
- Test real installs only in Windows Sandbox or a VM, never on the host without asking.
- Local git only. No remote until the user asks to publish.
- The user switches effort before coding phases; announce before starting a new phase.
