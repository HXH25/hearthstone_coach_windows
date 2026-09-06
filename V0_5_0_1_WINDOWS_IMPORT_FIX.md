# HearthCoach V0.5.0.1 Windows import hotfix

This hotfix addresses the first Windows compile failure reported by the user with `windows-sys 0.59`.

## Fixed

- `HMENU` is imported from `Win32::UI::WindowsAndMessaging`, where `windows-sys 0.59` defines it.
- `UpdateWindow` is imported from `Win32::Graphics::Gdi`, where `windows-sys 0.59` exposes it.
- Removed one harmless `unused_assignments` warning in the Trinket overlay branch.
- Control Center title bumped to V0.5.0.1 and Cargo package version to `0.5.0-hotfix.1`.

## Verify on Windows

```powershell
powershell -ExecutionPolicy Bypass -File .\VERIFY_V0_5_0_1.ps1
```

The script stops immediately on the first non-zero exit code.
