# HearthCoach V0.5.0.2 — One-click Windows Launcher

## 用户入口

普通用户以后只需要双击项目根目录：

```text
HearthCoach Launcher.cmd
```

不再要求用户先学会 `cargo`、PowerShell 执行策略或手工设置 Rust PATH。

## 启动器做什么

1. 把工作目录固定到项目根目录。
2. 检查 `cargo.exe` / `rustc.exe`。
3. 如果没有 Rust，先检查默认 Windows/MSVC 所需的 C++ Build Tools：
   - 使用 Visual Studio `vswhere.exe` 检查 C++ Build Tools；
   - 如果缺失，从微软官方 `https://aka.ms/vs/17/release/vs_BuildTools.exe` 下载安装器；
   - 请求安装 `Microsoft.VisualStudio.Workload.VCTools --includeRecommended`；
   - Windows 可能弹 UAC，这是系统要求，启动器不能绕过。
4. 从 `https://win.rustup.rs/x86_64` 下载官方 `rustup-init.exe`，安装 `x86_64-pc-windows-msvc` stable Rust（minimal profile），并把 `%USERPROFILE%\.cargo\bin` 加入本次启动器进程的 PATH。
5. 补装 `rustfmt`，再检查实际 Rust host；如果用户原本已有 MSVC Rust，也会确认 C++ Build Tools 可用。
6. 只有源码比现有 Release EXE 新、EXE 不存在或用户传入 `-ForceRebuild` 时才执行：

```text
cargo build --release --bin hearthcoach
```

7. 启动：

```text
target\release\hearthcoach.exe
```

默认隐藏后台 console，把 stdout / stderr 写到：

```text
logs\hearthcoach.stdout.log
logs\hearthcoach.stderr.log
```

HDT 风格的 HearthCoach Control Center 仍然正常显示；游戏内仍然只显示既有 Overlay。

## 可选参数

在 PowerShell / CMD 中也可以：

```powershell
.\HearthCoach Launcher.cmd -ForceRebuild
```

强制重新编译。

```powershell
.\HearthCoach Launcher.cmd -KeepConsole
```

保留 HearthCoach 的 console 窗口，适合调试。

```powershell
.\HearthCoach Launcher.cmd -SkipBuildToolsInstall
```

如果缺 C++ Build Tools 时不要自动下载安装，而是直接报错。

## 说明

这个启动器解决的是“源码分发”的一键启动。第一次编译仍会下载 Rust crates，因此需要联网，并且会比后续启动慢得多。

如果以后要给完全不懂开发环境的普通玩家分发，进一步建议在 CI/Release 中预编译 `hearthcoach.exe`，那样最终用户连 Rust 都不需要安装；本课程当前要求核心 Rust 实现，因此保留源码 + 自动 bootstrap 方案也便于验收。
