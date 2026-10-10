# croxz runtimes

Windows runtime package installer. Downloads and silently installs common runtimes (VC++, .NET, DirectX, WebView2, etc.) via a Material You GUI.

## Prerequisites

- Windows 10/11 x64
- Internet connection (required on launch)
- Node.js 18+, Rust 1.75+

## Installation (development)

```bash
npm install
npm run tauri dev
```

## Build

```bash
npm run tauri build
```

Output: `src-tauri/target/release/croxz-aio-runtimes.exe`

## Package Manifest

`packages/manifest.json` is served from GitHub raw:

```
https://raw.githubusercontent.com/bugragungoz/croxz_aio_runtimes/main/packages/manifest.json
```

To add Google Drive fallback for a package, set the `drive_url` field:

```json
{
  "drive_url": "https://drive.google.com/uc?export=download&id=YOUR_FILE_ID"
}
```

Packages with empty `url` and `drive_url` are shown as disabled in the UI.

## Usage

1. Launch `croxz-aio-runtimes.exe`
2. Select packages to install (recommended packages are pre-highlighted)
3. Click **Install** — a UAC prompt will appear
4. Approve the UAC prompt to begin silent installation
