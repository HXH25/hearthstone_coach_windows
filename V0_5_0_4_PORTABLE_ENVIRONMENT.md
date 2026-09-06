# HearthCoach V0.5.0.4 — Portable Environment Detection

This release fixes a clean/new-PC failure mode where the application could launch successfully but never recognize a Battlegrounds match because `hearthstone_dir` still pointed at the original developer machine.

## Detection order

Rust and the launcher use this order:

1. `HEARTHCOACH_HEARTHSTONE_DIR` when it points to a real `Hearthstone.exe`;
2. a currently running `Hearthstone.exe` process (authoritative for the game actually being played);
3. the saved `hearthstone_dir` when still valid;
4. common Hearthstone locations on available Windows drives.

A newly created config no longer assumes `D:\Hearthstone`.

## Power.log bootstrap

HearthCoach depends on Hearthstone's `Power.log`. V0.5.0.4 checks:

```text
%LOCALAPPDATA%\Blizzard\Hearthstone\log.config
```

and guarantees a compatible `[Power]` section:

```ini
[Power]
LogLevel=1
FilePrinting=True
ConsolePrinting=False
ScreenPrinting=False
Verbose=True
```

Other `log.config` sections are preserved. If the file is changed while Hearthstone is already running, the Control Center/launcher warns that Hearthstone must be restarted once before the setting takes effect.

This follows the same basic Power-log requirement used by HDT; HearthCoach does not need the user to copy a `Power.log` path manually.

## Control Center diagnostics

The new **环境诊断 / Environment Diagnostics** page shows:

```text
Hearthstone    OK / MISSING + resolved directory
Detection      env / saved config / running process / standard path
HDT / Card DB  authoritative catalog source
log.config     FOUND / MISSING
Power logging  READY / NEEDS REPAIR
Logs directory resolved path
Power.log      newest path
Last update    age + whether it is updating recently
```

Buttons:

```text
重新检测
自动检测并修复
```

## Dynamic monitor source

While no match is active, the Power.log watcher periodically yields back to the outer monitor loop. This is intentional: if Hearthstone was moved, installed after HearthCoach started, or the path was repaired from Control Center, the monitor adopts the new source within a few seconds instead of remaining attached to a stale path forever.

If no `Power.log` exists yet, that empty source also yields periodically; this lets a game started later from a different install be discovered instead of pinning the watcher to an empty old `Logs` directory.

## One-click launcher

`HearthCoach Launcher.cmd` now performs:

```text
Rust/MSVC -> HDT/HearthDb -> Hearthstone path -> Power logging -> build/start
```

If an unusual custom Hearthstone path is not found before the game starts, HearthCoach stays usable. Launch Hearthstone and the running-process detector will resolve it automatically; the Environment page can also force a repair immediately.
