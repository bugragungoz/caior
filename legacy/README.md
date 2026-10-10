# Legacy versions (reference only)

- `v1-tray-ps/` - `Launch.vbs` + `TrayInstaller.ps1`. Was packed with `runtimes/` into a WinRAR SFX (`_local/builds/croxz_aio_runtimes_v1_sfx.exe`, 943 MB). Offline; tray balloon progress; no elevation, arch or "already installed" checks.
- `v2-tauri/` - Tauri 2 + React app. Online only (manifest from GitHub raw), Material UI, needs WebView2. `src-tauri/target` (about 4 GB of build cache) and `node_modules` are still inside; they are safe to delete.
- `README.v2.md` - the v2 README.

Useful bits carried into v3: package list and silent args (v1 `TrayInstaller.ps1`, v2 `packages/manifest.json`), registry checks (v2 `src-tauri/src/commands/registry.rs`), download with fallback (v2 `installer.rs`).
