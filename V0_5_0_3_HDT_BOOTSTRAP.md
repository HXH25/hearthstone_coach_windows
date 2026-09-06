# HearthCoach V0.5.0.3 — HDT dependency bootstrap

V0.5.0.2 could install Rust/MSVC but still failed at runtime on a clean PC when HDT was absent:

```text
HearthCoach error: could not find HDT's HearthDb.dll
```

V0.5.0.3 makes this dependency part of the one-click bootstrap.

## Normal user flow

Double-click:

```text
HearthCoach Launcher.cmd
```

The launcher now checks, in order:

1. Visual Studio C++ Build Tools when MSVC Rust needs them;
2. rustup + stable Rust;
3. build/rebuild `target\\release\\hearthcoach.exe` when needed;
4. an existing HDT `HearthDb.dll`;
5. if missing, install official Hearthstone Deck Tracker;
6. export `HEARTHCOACH_HEARTHDB_DLL` for the child process;
7. start HearthCoach Control Center.

## HDT lookup

The launcher checks an explicit `HEARTHCOACH_HEARTHDB_DLL` first, then common user installs under `%LOCALAPPDATA%\\HearthstoneDeckTracker`, `%APPDATA%\\HearthstoneDeckTracker`, Program Files, and Chocolatey.

## Automatic HDT installation

Preferred route:

```powershell
winget install --id HearthSim.HearthstoneDeckTracker --exact --silent --accept-package-agreements --accept-source-agreements
```

If winget is unavailable or fails, the launcher queries the official HearthSim GitHub `releases/latest` API, downloads the official Windows installer asset, and runs it with `--silent`.

The launcher does **not** bundle or redistribute `HearthDb.dll` itself.

## Advanced opt-out

```powershell
.\\HearthCoach Launcher.cmd -SkipHdtInstall
```

Use this only when you provide one of:

```text
HEARTHCOACH_HEARTHDB_DLL=C:\\...\\HearthDb.dll
```

or a pre-exported catalog:

```text
HEARTHCOACH_CARD_DB=C:\\...\\hearthdb_cards_v4_zhCN.json
```

## Manual cargo run

`cargo run --bin hearthcoach` remains a developer path and does not install third-party dependencies automatically. If HDT is not installed, run the launcher once first or set `HEARTHCOACH_HEARTHDB_DLL` manually.
