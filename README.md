# CAIOR - croxz all-in-one runtimes

One small Windows app to set up every common runtime after a fresh install: Visual C++, .NET, DirectX, WebView2, Java and more.

- **Install now** - downloads from official sources, verifies each file and installs silently. You pick the packages, or use a profile.
- **Create offline package** - downloads and verifies, but does not install. Writes a single portable `.exe` (or a folder) you can keep on a USB drive and use on any PC without internet.
- Progress stays visible in a small card in the bottom-right corner.

> Status: alpha. The GUI and the command line both install packages (`caior.exe`, or `caior install --profile recommended --silent`). The offline package builder is next. See [docs/PLAN.md](docs/PLAN.md).

## Repository layout

| Path | What |
| --- | --- |
| `docs/` | Plan, package catalog, manifest format, UI design |
| `design/bugra/` | Local copy of the Bugra design system used by the UI |
| `assets/` | App icon |
| `crates/`, `xtask/` | Rust code: core engine, the `caior` app, developer tools |
| `catalog/` | Package catalog, one TOML file per category |
| `ui/`, `i18n/` | Slint UI (Bugra design), fonts, icons, Turkish translation |
| `tools/sandbox/` | Windows Sandbox install test |
| `legacy/` | Earlier versions (v1 PowerShell tray installer, v2 Tauri app), kept for reference |

Installer binaries are never committed. They are downloaded from their vendors at run time or when you build an offline package.
