# HearthCoach Project Log — Demo V0.3

## 2026-09-02 — HDT Knowledge + Overlay Diagnostics

### 用户确认的两个问题

1. DeepSeek V4 Flash 自身的酒馆战棋知识过旧，需要以 HDT 最新事实库驱动。
2. V0.2.1 实机没有看到卡牌推荐框，需要把“0 命中”和“Overlay 绘制失败”解耦诊断。

### V0.3 实现

- Harness parser/runtime 主干保持不变。
- HearthDb cache 升级 v4。
- exporter 读取最新 HDT CardDefs 后导出 `BaconPoolMinions` 成员标记。
- 新增 `HdtKnowledgeBase`：当前池 + AvailableTribes 过滤 + factual prompt + CardId validation。
- DeepSeek 不再把 `Cards.All` 前 N 条当卡池，不再任意截断当前相关随从池。
- Composition 新增 `core_card_ids`，必须通过当前 HDT 池验证。
- Watchlist 只允许当前合法池内随从；HDT 决定名称、Tier、文本，DeepSeek 只决定优先级/用途/原因。
- 右下角新增 `商店 / 推荐 / 命中 / HDT池` 诊断。
- 新增“测试框选”模式：Recruit 直接框第 1/3 张牌，不依赖 AI。
- Web debug page 同步增加诊断与测试框按钮。

### 不在本版范围

- 不扩展 Meta/胜率第三方数据源。
- 不把酒馆法术/饰品混进当前 BaconPoolMinions Watchlist。
- 不改最终 Agent 的 streaming/cancel/budget 架构。


## Demo V0.4.2

- Time-aware card valuation: low-tier / old-stage Watchlist value decays with round curve.
- Upgrade posture is soft (`prefer/neutral/delay`), with deterministic tier-curve pressure.
- ChoiceUpdated reclassifies Trinket/DarkGift as source/options arrive.
- Fixed-pitch shop overlay + revision fence + visual settle delay.
- In-game Panel V2 split into Decision / Guide pages.


## V0.4.3 compact overlay

- compact bottom-right decision card
- horizontal top composition guide with best-effort local card art
- Trinket choice temporarily replaces tactical action list
- increased overlay opacity
- strategic JSON missing target fields use Rust defaults

## V0.4.4 auto guide / movable panel

- Top composition guide now follows `current_stage` automatically; manual stage override was removed from the visible guide.
- Decision panel title bar is draggable inside the Hearthstone client.
- Bottom-right resize handle supports live panel resizing.
- Position/size persist as normalized overlay config ratios.
- `复位` restores compact bottom-right defaults.


## V0.4.5 hover + dialogue

- 顶部 Guide card hover tooltip：显示阵容中的具体作用与 reason。
- RoundPlan 小目标默认折叠，hover 展开，不永久占用决策框。
- 新增局内 AI chat；`RoundPlanPatch` 只修改用户明确要求修改的字段，Rust 校验范围并阻止旧 phase 响应覆盖新计划。
- 用户对话修改计划后提升 `planner_generation`，旧慢规划结果自动失效，然后立即 `recompute_tactical()`。

## V0.4.6 concrete trinkets + UI input correctness

- Open Trinket choices are materialized into concrete candidate views and ranked only among real candidates.
- Guide card-art loading now rejects broad HDT sprite matches and uses aspect-safe center cropping.
- Compact panel strings wrap against measured GDI pixel width.
- AI chat moved away from a layered parent and no longer repaints over the native EDIT control each overlay tick.


## V0.4.7 grounded chat + text layout

- Native scrollable chat history.
- Authoritative current-round validation.
- HDT CardId grounding/citations.
- TacticalPlan supplied to chat.
- Small goal hover uses pixel wrapping across the full content area.


## V0.5.0 — HDT-style Control Center / Course Compliance

- 新增 native Windows `HearthCoach Control Center`；游戏内仍只展示轻量 Overlay。
- R3：桌面 UI 配置 provider compatibility、Endpoint、API Key、Model、Context Window、Thinking、Timeout、Token价格和预算。
- R4：Rust `TaskManager` 管理所有 LLM 长任务；阶段/百分比/耗时实时渲染；`reqwest + tokio::select!` 支持真实 HTTP cancel。
- R5：`AgentSessionArchive` 自动/手动保存 GameArchive + Plan + Replan + Chat + Tasks + API Calls + Agent Trace，可在桌面 History 查看/加载/恢复 Agent 上下文。
- R6：每个物理 API 请求记录 provider `response.usage`，按配置价格计算 cost；Token/Cost Dashboard 清晰展示；预算达到上限时取消剩余任务并拒绝新 LLM 任务。
- 新主入口：`cargo run --bin hearthcoach`。


## V0.5.0.3 one-click bootstrap

- Added `HearthCoach Launcher.cmd` as the normal-user entry point.
- Added `tools/bootstrap_launcher.ps1` to install official rustup/stable Rust when cargo is absent.
- For MSVC Rust, detects Visual Studio C++ Build Tools and can install the official VCTools workload with UAC.
- Builds `hearthcoach` in release mode only when needed, then launches the desktop Control Center while preserving the existing in-game overlay.
- stdout/stderr are redirected to `logs/` for startup diagnostics.


## V0.5.0.3 — HDT dependency bootstrap

- One-click launcher now treats HDT/HearthDb as a real prerequisite rather than letting the Rust binary fail after build.
- Searches `HEARTHCOACH_HEARTHDB_DLL`, LocalAppData/AppData HDT installs and common Chocolatey locations.
- If missing, installs official `HearthSim.HearthstoneDeckTracker` with winget; falls back to the latest official HearthSim GitHub Release installer.
- Exports the discovered HearthDb.dll path into the child HearthCoach process.
- `-SkipHdtInstall` is available for advanced/manual deployments.


## V0.5.0.4 — portable environment detection

- Removed the machine-specific default `D:\Hearthstone`; new configs start with an empty path and are auto-resolved.
- Added Rust `demo::environment`: explicit env override -> running Hearthstone process -> saved valid path -> common install paths.
- Launcher and Rust core both repair the `[Power]` section of `%LOCALAPPDATA%\Blizzard\Hearthstone\log.config` without deleting unrelated sections.
- If logging was repaired while Hearthstone is running, users are told to restart Hearthstone once.
- Control Center adds an Environment Diagnostics page showing Hearthstone path, HDT/Card DB source, log.config, Power.log and last update age.
- The monitor periodically relinquishes an inactive source so moved/new installs can be re-detected instead of sticking forever to an old machine path.

## V0.5.0.5 — Unicode-safe process discovery

- Fixed a cleanly reproduced Windows portability bug for paths such as `D:\应用\Hearthstone`.
- Removed the Rust runtime dependency on PowerShell stdout for Hearthstone executable discovery.
- Runtime now enumerates processes through ToolHelp W APIs and reads the full executable path with `QueryFullProcessImageNameW`.
- The returned UTF-16 path is converted directly to `OsString`/`PathBuf`, so no console-codepage/UTF-8 boundary exists.
- `hearthstone_is_running()` now reuses native process enumeration rather than parsing `tasklist.exe` output.
- Added a Windows regression test for a Unicode Hearthstone path round-trip.

## V0.5.0.6 — robust card art + auto guide

- Top guide no longer depends on the first PNG found in HDT's generic Images tree. It prefers CardPortraits/CardTiles JPG caches and falls back deterministically.
- PNG card art is decoded through Rust `image` after removing the optional iCCP chunk, so malformed ICC metadata cannot suppress a guide card.
- One damaged cache file no longer poisons a CardId: every valid candidate is tried before text fallback.
- Added `agent.auto_prepare_guide=true`: when live available-tribe data first arrives, HearthCoach can automatically analyze compositions and generate the first validated watchlist.
- Empty guide states now explain whether the blocker is missing tribes, missing model config, composition analysis, or watchlist generation instead of rendering a large blank strip.


## V0.5.0.7 — latest Power.log + manual refresh

- 修复 V0.5.0.6 watcher 在旧客户端异常退出后仍保持 `runtime.is_match_active()==true`，从而拒绝切换到新 Power.log 的生命周期 bug。
- watcher 每轮扫描磁盘最新 Power.log；只要最新路径与当前跟踪路径不同，就封存旧 partial session 并立即返回外层重开最新日志，不再受 runtime active 状态阻塞。
- 新增显式 `HarnessEvent::MatchInterrupted { reason }`，中断会话保留 Round/Agent/API/Token 审计数据，同时清空 live state。
- Control Center 环境诊断新增“刷新日志”按钮，通过原子 generation 通知后台 watcher 立即退出并重新扫描；无需重启 HearthCoach。
- 环境诊断同时显示“当前监听日志”和“磁盘最新 Power.log”，不一致时给出警告。
