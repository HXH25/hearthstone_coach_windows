# HearthCoach V0.5.0.5 — Unicode-safe Hearthstone process discovery

V0.5.0.4 could fail on a valid custom Hearthstone installation such as:

```text
D:\应用\Hearthstone\Hearthstone.exe
```

even while `Get-Process Hearthstone | Select Name,Path` showed the correct path.

## Root cause

The Rust runtime asked a PowerShell child process for the executable path and decoded the child's stdout with `String::from_utf8_lossy`. Windows PowerShell stdout is not guaranteed to be UTF-8. Paths containing Chinese or other non-ASCII characters could therefore be corrupted before being converted back to `PathBuf`.

That bug was in HearthCoach's discovery bridge, not in Rust `PathBuf` itself.

## Fix

V0.5.0.5 removes PowerShell/tasklist from the Rust runtime discovery path.

The Windows implementation now uses native Win32 wide-character APIs:

```text
CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)
  -> Process32FirstW / Process32NextW
  -> locate Hearthstone.exe PID
  -> OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)
  -> QueryFullProcessImageNameW
  -> UTF-16 buffer
  -> OsString::from_wide
  -> PathBuf
```

No OEM/ANSI/UTF-8 text conversion is involved. Paths such as `D:\应用\Hearthstone` and other Unicode names are preserved end-to-end.

`hearthstone_is_running()` uses the same native process enumeration instead of parsing localized `tasklist.exe` output.

## Launcher

The PowerShell launcher already keeps discovered paths as .NET strings and writes them through .NET JSON/file APIs, so it did not have the same UTF-8 stdout bug. The important runtime fix is in `src/demo/environment.rs`, which must be able to discover Hearthstone after HearthCoach has already started.

## Verification

Run on Windows:

```powershell
powershell -ExecutionPolicy Bypass -File .\VERIFY_V0_5_0_5.ps1
```

A real-machine regression test should use a Hearthstone installation whose parent directory contains non-ASCII characters. With Hearthstone running, Control Center -> Environment should show:

```text
Hearthstone : OK
D:\应用\Hearthstone
检测来源     : running Hearthstone process (Win32 Unicode)
```
