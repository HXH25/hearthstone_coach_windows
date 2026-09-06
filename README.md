# HearthCoach for Windows

HearthCoach 是一个以 **Rust 为核心**的炉石传说：酒馆战棋实时决策辅助工具。

当前仓库对应 **Windows v0.5.0.7**。程序通过本地读取 Hearthstone 日志、HDT/HearthDb/HearthMirror 卡牌与种族数据，结合：

- **LLM 慢速战略规划**
- **Rust 高频战术评分**
- **Win32 游戏内 Overlay**
- **原生桌面 Control Center**

为当前对局提供阵容方向、关注卡表、回合目标、买牌/升本/刷新/冻结建议、饰品排序与 AI 对话。

> 本项目只读取本地日志与卡牌数据，并在游戏外创建 Overlay。  
> **不会注入 Hearthstone 进程，不会自动点击，不会自动买牌或操作游戏。**

---

## 目录

- [功能概览](#功能概览)
- [系统架构](#系统架构)
- [运行环境与前置要求](#运行环境与前置要求)
- [最快启动方式](#最快启动方式)
- [首次启动后要做什么](#首次启动后要做什么)
- [开发者启动方式](#开发者启动方式)
- [配置说明](#配置说明)
- [常用诊断命令](#常用诊断命令)
- [常见 Bug 与处理方法](#常见-bug-与处理方法)
- [如何提交 Bug](#如何提交-bug)
- [项目目录](#项目目录)
- [安全与隐私](#安全与隐私)

---

# 功能概览

## 1. 实时游戏状态读取

HearthCoach 从 Hearthstone 本地日志中增量读取并还原：

- 当前对局是否开始
- 招募 / 战斗阶段
- 当前回合
- 金币
- 酒馆等级
- 手牌
- 己方棋盘
- 当前商店
- 对手信息
- Choice / 饰品候选
- 玩家购买、出售、刷新、冻结、升本等动作
- 本局可用种族

核心实现位于：

```text
src/harness/
```

---

## 2. LLM 慢规划

大模型主要负责低频、长时间尺度的战略判断，包括：

- 推荐可玩阵容
- 构建前 / 中 / 后期 Watchlist
- 生成当前或下一轮 `RoundPlan`
- 规划饰品需求
- 对真实饰品候选排序
- AI 教练对话
- 根据明确用户要求返回 `RoundPlanPatch`

核心实现：

```text
src/demo/deepseek.rs
src/demo/model.rs
src/demo/hdt_knowledge.rs
```

---

## 3. Rust 快速战术决策

实时商店变化时，不需要每次等待大模型。

Rust 会将：

```text
实时 Snapshot
+
RoundPlan
+
Watchlist
```

转换成：

```text
Buy
Play
Sell -> Buy
Upgrade
Refresh
Freeze
Hold
```

等候选动作的实时评分，并生成 `TacticalPlan`。

核心实现：

```text
src/demo/agent.rs
```

---

## 4. Control Center

Windows 原生桌面控制中心用于：

- 模型 API 配置
- API Key
- Endpoint
- Model
- Context Window
- Thinking Mode
- Token / 成本预算
- 环境诊断
- Power.log 状态
- 手动刷新日志
- 历史对局
- Agent 轨迹
- API 调用
- Chat
- 长任务状态和取消

---

## 5. 游戏内 Overlay

游戏内 Overlay 提供：

- 顶部阵容指南
- 当前阶段 Watchlist
- 卡牌图片 / 文字 fallback
- 当前小目标
- 排名前几的实时动作
- 简短动作路线
- 饰品选择
- AI 对话
- 商店推荐框
- Overlay 测试框选

---

# 系统架构

```text
Hearthstone Power.log / Choice logs
                |
                v
        monitor / parser
                |
                v
        HarnessRuntime
                |
                v
 EntityStore / StateProjector
                |
                v
             Snapshot
                |
                v
      DemoState / Orchestrator
          /             \
         /               \
        v                 v
LLM Slow Planner    Rust Fast Agent
        |                 ^
        |                 |
        +---- RoundPlan ---+
        +---- Watchlist ---+
                          |
                          v
                     TacticalPlan
                          |
                          v
                  Control Center / Overlay
```

核心原则：

```text
HDT / HearthDb = 卡牌事实
LLM            = 战略推理
Rust           = 校验 + 实时评分 + 执行建议
```

---

# 运行环境与前置要求

## 必需环境

推荐：

- **Windows 10 / Windows 11 x64**
- Hearthstone 已安装
- 可以正常进入酒馆战棋
- 网络连接
- 一个可用的大模型 API
- 首次安装时允许下载 Rust / Visual Studio Build Tools / HDT

当前一键启动器使用：

```text
x86_64-pc-windows-msvc
```

因此主要面向 Windows x64。

---

## Rust

项目使用 Rust 2021 Edition。

如果电脑未安装 Rust：

```text
HearthCoach Launcher.cmd
```

会自动通过官方 `rustup` 安装 Stable Rust。

也可以自行安装后确认：

```powershell
rustc --version
cargo --version
```

---

## Microsoft Visual C++ Build Tools

Windows MSVC Rust 需要 Visual Studio C++ linker。

如果缺少 `link.exe` / MSVC Build Tools，一键启动器会尝试自动安装：

```text
Visual Studio 2022 Build Tools
Desktop development with C++
```

安装过程中 Windows 可能弹出 **UAC 管理员确认**。

如果安装器要求重启，请重启 Windows 后再次运行 Launcher。

---

## Hearthstone Deck Tracker

当前 Windows 版使用 HDT 提供的：

```text
HearthDb.dll
HearthMirror.dll
CardDefs.base.xml
```

用于：

- 当前酒馆卡池
- CardId / 卡名 / 文本 / 酒馆等级 / 种族
- 当前可用种族

如果没有 HDT，一键启动器会优先：

```text
winget
```

安装官方 Hearthstone Deck Tracker。

如果 winget 不可用，会尝试从 HearthSim 官方 GitHub Release 下载 HDT Installer。

> 某些新设备上，HDT 安装完成后需要先手动打开一次 HDT，再重新运行 HearthCoach Launcher，DLL 才能被正确发现。

---

## Hearthstone Power.log

程序依赖 Hearthstone 的 Power logging。

启动器 / Control Center 会检查并修复：

```text
%LOCALAPPDATA%\Blizzard\Hearthstone\log.config
```

如果 Hearthstone 在修复 `log.config` 时已经运行：

> 请关闭并重新启动 Hearthstone 一次。

否则新的 Power logging 配置不会立即生效。

---

# 最快启动方式

## 方式 A：一键启动，推荐普通用户

克隆或下载项目后，直接双击：

```text
HearthCoach Launcher.cmd
```

启动器会依次处理：

```text
检查 Rust
    ↓
缺少则安装 rustup + stable
    ↓
检查 MSVC Build Tools
    ↓
缺少则安装 C++ workload
    ↓
检测 / 安装 HDT
    ↓
检测 Hearthstone
    ↓
检查 / 修复 Power.log 配置
    ↓
编译 release 版本
    ↓
启动 HearthCoach
```

首次启动可能需要数分钟。

之后如果源码没有变化，Launcher 不会无条件重新编译整个项目。

---

# 首次启动后要做什么

## 1. 打开“模型配置”

在 Control Center 中进入：

```text
模型配置
```

填写：

- API Compatibility
- Endpoint
- API Key
- Model
- Context Window
- Max Output Tokens
- Timeout
- Thinking Mode
- Token 价格
- Token / Cost Budget

然后点击：

```text
保存配置
```

---

## 2. API Compatibility

当前支持两种主要模式。

### DeepSeek

```text
api_compatibility = deepseek
```

程序会发送 DeepSeek 风格的 thinking 配置。

### OpenAI-compatible

```text
api_compatibility = openai
```

适用于兼容：

```text
/chat/completions
```

的第三方服务，并避免发送 DeepSeek 专用 thinking 字段。

---

## 3. Endpoint 怎么填

程序会自动拼接：

```text
{base_url}/chat/completions
```

例如：

```text
https://api.deepseek.com
```

最终请求：

```text
https://api.deepseek.com/chat/completions
```

如果你的服务要求：

```text
https://example.com/v1/chat/completions
```

那么 Base URL 应填：

```text
https://example.com/v1
```

不要把 `/chat/completions` 重复填进去。

---

## 4. 测试模型

配置完成后点击：

```text
测试连接
```

如果成功，再开始正式对局。

---

## 5. 环境诊断

进入：

```text
环境诊断
```

确认至少：

- Hearthstone 路径正确
- Power logging ready
- 当前监听 Power.log 正常
- 磁盘最新 Power.log 与监听日志一致

有问题时先点击：

```text
自动检测并修复
```

必要时：

```text
刷新日志
```

---

# 开发者启动方式

## 1. 克隆项目

```powershell
git clone https://github.com/HXH25/hearthstone_coach_windows.git
cd hearthstone_coach_windows
```

---

## 2. 运行完整验证

```powershell
powershell -ExecutionPolicy Bypass -File .\VERIFY_V0_5_0_7.ps1
```

验证内容包括：

- `cargo fmt`
- `cargo fmt --check`
- `cargo check --bin hearthcoach`
- `cargo check --bin hearthcoach_demo`
- `cargo test`
- V0.5.0.7 Power.log watcher regression checks
- Environment / Unicode path checks
- Overlay card-art checks

正常结束时应看到：

```text
V0.5.0.7 verification passed.
```

---

## 3. 手工构建

Debug：

```powershell
cargo build
```

Release：

```powershell
cargo build --release --bin hearthcoach
```

生成：

```text
target\release\hearthcoach.exe
```

---

## 4. 手工运行

```powershell
cargo run --bin hearthcoach
```

或 release：

```powershell
.\target\release\hearthcoach.exe
```

推荐日常使用仍然通过：

```text
HearthCoach Launcher.cmd
```

因为 Launcher 会额外做环境检查。

---

# 配置说明

第一次运行会创建：

```text
hearthcoach_demo.json
```

示例模板：

```text
hearthcoach_demo.example.json
```

真实配置文件已在 `.gitignore` 中忽略。

其中可能包含：

```text
API Key
模型 Endpoint
个人预算配置
本机 Hearthstone 路径
Overlay 布局
```

**不要把真实 `hearthcoach_demo.json` 上传到 GitHub。**

---

## 常用环境变量

### 手工指定 Hearthstone

```powershell
$env:HEARTHCOACH_HEARTHSTONE_DIR = "D:\Games\Hearthstone"
```

---

### 手工指定 HearthDb.dll

```powershell
$env:HEARTHCOACH_HEARTHDB_DLL = "C:\...\HearthDb.dll"
```

---

### 手工指定 HearthMirror.dll

```powershell
$env:HEARTHCOACH_HEARTHMIRROR_DLL = "C:\...\HearthMirror.dll"
```

---

### 手工指定 CardDefs

```powershell
$env:HEARTHCOACH_CARDDEFS_BASE = "C:\...\CardDefs.base.xml"
```

---

### 使用外部 Card DB

```powershell
$env:HEARTHCOACH_CARD_DB = "C:\path\to\cards.json"
```

---

### 使用其他配置文件

```powershell
$env:HEARTHCOACH_DEMO_CONFIG = "C:\path\to\hearthcoach_demo.json"
```

---

# 常用诊断命令

## Card DB 状态

```powershell
cargo run -- carddb status
```

正常应看到：

```text
HDT HearthDb.dll : ...
HDT CardDefs      : ...
Rust cache        : ...\hearthdb_cards_v4_zhCN.json
Cards             : ...
Bacon pool minions: > 0
Source            : ...
```

如果：

```text
Bacon pool minions: 0
```

请不要先排查 AI。

先解决 HDT / HearthDb / CardDefs。

---

## 强制重建卡库

```powershell
cargo run -- carddb rebuild
```

适合：

- 游戏更新后
- HDT 更新后
- 卡牌数据明显过旧
- Card DB cache 损坏

---

## 查询单张 CardId

```powershell
cargo run -- card BGXXXXXXXX
```

可以查看：

- Name
- Type
- Tribes
- Tavern Tier
- Cost
- Attack / Health
- Golden
- 是否在当前 BG pool
- Mechanics
- Text

---

## 测试本局可用种族

请在酒馆战棋大厅 / 对局实际处于有效状态时执行：

```powershell
cargo run -- tribes status
```

正常应看到：

```text
HearthMirror.dll : ...
Available tribes : [...]
```

---

## 手工监听一局

```powershell
cargo run -- watch
```

也可以指定：

```powershell
cargo run -- watch --hearthstone-dir "D:\Games\Hearthstone"
```

---

## 回放 Power.log

```powershell
cargo run -- replay "C:\path\to\Power.log"
```

适合离线排查日志解析问题。

---

# 常见 Bug 与处理方法

# 1. 双击 Launcher 后失败

首先不要马上关闭命令行窗口。

Launcher 本身在失败时会：

```text
pause
```

请保留完整错误文本。

同时检查：

```text
logs\hearthcoach.stdout.log
logs\hearthcoach.stderr.log
```

---

# 2. `cargo` / `rustc` 找不到

推荐重新运行：

```text
HearthCoach Launcher.cmd
```

Launcher 会自动安装 Rust。

手工确认：

```powershell
rustc --version
cargo --version
```

如果刚安装完成当前 PowerShell 仍找不到，可以：

- 重新打开 PowerShell
- 或重新登录 Windows
- 或检查 `%USERPROFILE%\.cargo\bin` 是否在 PATH

---

# 3. `link.exe` 找不到 / MSVC 编译失败

典型错误：

```text
linker `link.exe` not found
```

重新运行 Launcher。

它会检查并安装：

```text
Visual Studio 2022 Build Tools
C++ workload
```

如果安装器提示需要重启：

1. 重启 Windows
2. 再运行 Launcher

---

# 4. 找不到 HDT / HearthDb.dll

执行：

```powershell
cargo run -- carddb status
```

如果提示：

```text
could not find HDT's HearthDb.dll
```

处理顺序：

1. 确认 Hearthstone Deck Tracker 已安装
2. 手动启动 HDT 一次
3. 完全退出 HDT
4. 重新运行 `HearthCoach Launcher.cmd`

仍然失败时手工设置：

```powershell
$env:HEARTHCOACH_HEARTHDB_DLL = "完整的 HearthDb.dll 路径"
```

然后：

```powershell
cargo run -- carddb status
```

---

# 5. 找不到 HearthMirror.dll / 无法检测本局种族

先执行：

```powershell
cargo run -- tribes status
```

如果找不到 HearthMirror：

1. 确认 HDT 已正确安装
2. 启动 HDT 一次
3. 确认 HDT 安装目录中存在 `HearthMirror.dll`

必要时：

```powershell
$env:HEARTHCOACH_HEARTHMIRROR_DLL = "C:\...\HearthMirror.dll"
cargo run -- tribes status
```

`tribes status` 最好在酒馆战棋大厅或正在进行的对局中测试。

---

# 6. 检测不到 Hearthstone

进入 Control Center：

```text
环境诊断
→ 自动检测并修复
```

如果游戏安装在非标准目录：

先启动 Hearthstone，再点击自动检测。

V0.5.0.7 使用 Win32 Unicode API 获取运行中的 Hearthstone 路径，可以处理中文 / Unicode 安装目录。

也可以手工指定：

```powershell
$env:HEARTHCOACH_HEARTHSTONE_DIR = "D:\应用\Hearthstone"
```

---

# 7. Power.log 不更新

先打开：

```text
Control Center
→ 环境诊断
```

检查 Power logging 状态。

如果程序刚修复：

```text
%LOCALAPPDATA%\Blizzard\Hearthstone\log.config
```

而 Hearthstone 当时已经运行：

> 关闭并重新启动 Hearthstone。

---

# 8. HearthCoach 一直卡在旧 Power.log

这是 V0.5.0.7 重点修复的问题。

先在：

```text
环境诊断
```

查看：

- 当前监听日志
- 磁盘最新 Power.log

如果不一致，点击：

```text
刷新日志
```

V0.5.0.7 会强制重置 live watcher 并重新扫描最新 Power.log。

如果仍然有问题：

1. 关闭 Hearthstone
2. 重新启动 Hearthstone
3. 进入一局新的酒馆战棋
4. 点击“刷新日志”
5. 检查 `logs\hearthcoach.stderr.log`

---

# 9. 新开一局后还显示上一局状态

V0.5.0.7 会在发现新的 Power.log 时切换 source。

如果仍有残留：

```text
环境诊断
→ 刷新日志
```

并确认：

```text
当前监听 Power.log
```

已经指向最新 session。

如果问题可复现，请保留：

- 新旧 Power.log 路径
- `hearthcoach.stderr.log`
- 对应 `hearthcoach_sessions` 会话文件

用于定位。

---

# 10. Overlay 完全没有推荐框

先不要排查 AI。

在 Recruit 阶段点击：

```text
测试框选：框第1/3张
```

### 如果测试框也不显示

问题更可能在：

```text
Overlay
窗口坐标
卡槽坐标
显示层
```

尝试：

- 确认游戏不是异常全屏模式
- 调整 Overlay 布局
- 使用“复位”
- 检查屏幕缩放 / 分辨率
- 查看 stderr

### 如果测试框能显示，但正常框没有

观察右下角：

```text
商店 N · 推荐 N · 命中 N
```

如果：

```text
推荐 > 0
命中 = 0
```

说明当前商店没有 Watchlist 命中，不是 Overlay 坏了。

如果：

```text
推荐 = 0
```

继续检查：

- 是否已经生成阵容指南
- 模型调用是否成功
- Watchlist 是否生成
- Card DB 是否正常

---

# 11. 顶部阵容指南没有显示

依次检查：

1. 是否存在 Selected Composition
2. 是否已经成功生成 Watchlist
3. 模型测试是否通过
4. Card DB 是否正常
5. `推荐 N` 是否大于 0

自动准备阵容指南默认：

```json
"auto_prepare_guide": true
```

如果模型不可用，指南可能无法正常生成。

---

# 12. `libpng warning: iCCP: known incorrect sRGB profile`

V0.5.0.6+ 已加入 PNG iCCP 清理逻辑。

一般而言：

```text
单张卡图存在异常
```

不应该阻断整个阵容指南，程序应回退到其他可信卡图或文字卡。

因此，如果：

```text
整个 Guide 都没有显示
```

不要只盯着 iCCP warning。

应优先检查：

```text
Watchlist
Selected Composition
CardId
模型调用
```

---

# 13. 卡牌图片错误 / 不显示

程序优先使用：

```text
HDT CardPortraits
HDT CardTiles
```

并对图片做可信文件名和比例检查。

找不到可靠图片时会自动回退到：

```text
文字卡
```

这不是致命错误。

如果图片错位：

- 检查 HDT 图片缓存
- 检查 CardId
- 检查当前 HDT 是否为最新版本

---

# 14. API 测试失败

先进入：

```text
模型配置
```

检查：

- API Compatibility
- Endpoint
- API Key
- Model
- Context Window
- Timeout

然后点击：

```text
测试连接
```

### 401 / Unauthorized

通常是：

```text
API Key 错误
```

### 404 / Not Found

常见原因：

- Endpoint 写错
- 重复填写了 `/chat/completions`
- Model ID 不存在
- Provider 路由格式不同

### Timeout

可以：

- 提高 Timeout Seconds
- 换更快模型
- 检查网络 / API 服务状态
- 暂时关闭 Thinking

---

# 15. AI 有回复，但卡牌信息明显过时

先不要改 Prompt。

执行：

```powershell
cargo run -- carddb status
cargo run -- carddb rebuild
```

当前设计原则：

```text
HDT = 事实
LLM = 推理
```

如果卡池事实本身旧了，应先更新：

```text
HDT
CardDefs
Rust cache
```

---

# 16. AI 对话能回答，但建议和实时局面不同步

确认：

- 当前是否处于最新一局
- 当前 Power.log 是否是最新
- 商店 revision 是否持续变化
- 模型计划是否属于当前回合

如果怀疑日志卡住：

```text
环境诊断
→ 刷新日志
```

---

# 17. 程序启动后立即退出

检查：

```text
logs\hearthcoach.stderr.log
```

也可以直接保留控制台：

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\bootstrap_launcher.ps1 -KeepConsole
```

或者：

```powershell
cargo run --bin hearthcoach
```

这样可以直接看到 Rust 错误。

---

# 18. 中文安装目录导致检测失败

V0.5.0.5 已将 Hearthstone 运行路径检测改为 Win32 UTF-16 API。

例如：

```text
D:\应用\Hearthstone
```

应该可以被正确识别。

仍有问题时：

```powershell
$env:HEARTHCOACH_HEARTHSTONE_DIR = "D:\应用\Hearthstone"
```

并在 Issue 中附上路径形式，但不要包含隐私信息。

---

# 19. Overlay 位置不合适

右下角面板：

- 拖动标题栏移动
- 拖动 `↘` 缩放
- 点击 `复位` 恢复默认

布局会保存在：

```text
hearthcoach_demo.json
```

---

# 20. 怀疑代码版本有问题

首先执行：

```powershell
powershell -ExecutionPolicy Bypass -File .\VERIFY_V0_5_0_7.ps1
```

如果 Verification 都无法通过，请先解决编译 / 测试问题，再开始实战排查。

---

# 如何提交 Bug

建议在 GitHub Issue 中提供下面的信息。

## 基础环境

```text
HearthCoach version:
Windows version:
Hearthstone version/build:
HDT version:
Rust version:
显示分辨率 / Windows 缩放:
```

---

## 问题类型

注明属于：

```text
[Environment]
[Power.log]
[Parser]
[Runtime]
[CardDB]
[Tribes]
[LLM]
[Agent]
[Overlay]
[Guide]
[Chat]
[Launcher]
```

---

## 必要日志

优先提供：

```text
logs\hearthcoach.stdout.log
logs\hearthcoach.stderr.log
```

如果是状态解析问题，可以附：

```text
对应的 Power.log 片段
```

如果是 Agent / 计划问题，可以附：

```text
hearthcoach_sessions\<session>.json
```

---

## 诊断命令输出

建议同时提供：

```powershell
cargo run -- carddb status
cargo run -- tribes status
powershell -ExecutionPolicy Bypass -File .\VERIFY_V0_5_0_7.ps1
```

---

## 请不要上传

**绝对不要在 Issue 中上传：**

```text
API Key
完整 hearthcoach_demo.json（如果包含真实 Key）
任何账号密码
私人 Token
```

如果必须贴配置：

> 请先把 `api_key` 替换成 `REDACTED`。

---

# 项目目录

```text
.
├── Cargo.toml
├── HearthCoach Launcher.cmd
├── hearthcoach_demo.example.json
├── src/
│   ├── bin/
│   │   ├── hearthcoach.rs
│   │   └── hearthcoach_demo.rs
│   ├── demo/
│   │   ├── agent.rs
│   │   ├── compliance.rs
│   │   ├── config.rs
│   │   ├── control_center.rs
│   │   ├── deepseek.rs
│   │   ├── environment.rs
│   │   ├── hdt_knowledge.rs
│   │   ├── model.rs
│   │   ├── overlay.rs
│   │   └── server.rs
│   └── harness/
│       ├── archive.rs
│       ├── available_tribes.rs
│       ├── card_catalog.rs
│       ├── monitor.rs
│       ├── router.rs
│       ├── runtime.rs
│       ├── choice/
│       ├── power/
│       └── state/
├── tests/
├── tools/
│   ├── bootstrap_launcher.ps1
│   ├── export_hearthdb.ps1
│   └── read_available_tribes.ps1
├── web/
└── VERIFY_V0_5_0_7.ps1
```

---

# 核心代码阅读顺序

如果要理解 Agent，推荐：

```text
src/demo/model.rs
    ↓
src/demo/deepseek.rs
    ↓
src/demo/server.rs
    ↓
src/demo/agent.rs
```

如果要理解日志状态还原：

```text
src/harness/monitor.rs
    ↓
src/harness/router.rs
    ↓
src/harness/power/parser.rs
    ↓
src/harness/runtime.rs
    ↓
src/harness/state/entity_store.rs
    ↓
src/harness/state/snapshot.rs
```

---

# 测试

完整 Rust 测试：

```powershell
cargo test
```

主要测试文件：

```text
tests/parser_tests.rs
tests/runtime_tests.rs
tests/agent_tests.rs
tests/demo_state_tests.rs
tests/card_catalog_tests.rs
tests/hdt_knowledge_tests.rs
tests/compliance_tests.rs
```

---

# 版本

当前：

```text
HearthCoach Windows v0.5.0.7
Cargo package: 0.5.0-hotfix.7
```

V0.5.0.7 主要修复：

- 新 Power.log 自动切换
- 旧 active runtime 不再阻止 source rotation
- Control Center 手动“刷新日志”
- Unicode Hearthstone 路径支持
- HDT 卡图 / PNG iCCP 兼容
- 自动准备阵容指南

相关文档：

```text
V0_5_0_7_POWER_LOG_REFRESH.md
V0_5_0_6_CARD_ART_GUIDE_FIX.md
V0_5_0_5_UNICODE_PATH_FIX.md
V0_5_0_4_PORTABLE_ENVIRONMENT.md
V0_5_0_COURSE_COMPLIANCE.md
PROJECT_LOG.md
```

---

# Git 与本地文件

建议不要提交下面这些运行时文件：

```text
target/
hearthcoach_demo.json
hearthcoach_sessions/
records*/
logs/
hearthdb_cards_v4_zhCN.json
*.log
```

推荐 `.gitignore`：

```gitignore
/target/
/hearthcoach_demo.json
/hearthcoach_sessions/
/records*/
/logs/
/hearthdb_cards_v4_zhCN.json
*.log
.DS_Store
```

---

# 安全与隐私

HearthCoach 会在本地处理：

- Hearthstone Power.log
- 卡牌数据库
- Agent Session
- AI Chat
- 模型 API 调用统计

大模型请求会发送与当前策略分析相关的游戏状态到你配置的 API Provider。

API Key 默认保存在本地：

```text
hearthcoach_demo.json
```

该文件应始终保持 Git ignored。

---

# License

`Cargo.toml` 当前声明：

```text
MIT
```

如果公开发布本仓库，建议同时在仓库根目录加入正式 `LICENSE` 文件。

---

# Disclaimer

Hearthstone 及相关名称、图片与游戏资源归其相应权利人所有。

本项目为独立的实验性工具，与 Blizzard Entertainment、HearthSim 或 Hearthstone Deck Tracker 官方无隶属关系。
