# HearthCoach V0.5.0

> V0.5.0 新增 HDT 式原生桌面 Control Center，并补齐课程 R3–R6。游戏内继续使用 V0.4.7 Overlay。主入口改为 `cargo run --bin hearthcoach`。完整说明见 `V0_5_0_COURSE_COMPLIANCE.md`。

---

# HearthCoach DeepSeek Demo V0.4.7

## 本版只修两个问题

### 1. DeepSeek 酒馆战棋知识太老

V0.2.x 虽然读取了 HearthDb，但给模型的“卡池”仍然来自 `Cards.All` 的筛选结果，而且还会排序后截断。V0.3 改为真正使用 HDT/HearthDb 当前维护的：

```text
HearthDb.Cards.BaconPoolMinions
```

程序启动时会：

1. 定位本机最新 HDT `HearthDb.dll`；
2. 优先加载 `%APPDATA%\HearthstoneDeckTracker\CardDefs\CardDefs.base.xml`；
3. 重新读取 `Cards.BaconPoolMinions`；
4. 导出到新的 Rust cache：`hearthdb_cards_v4_zhCN.json`；
5. Rust `HdtKnowledgeBase` 再按本局开放种族过滤当前池；
6. 把 **全部相关当前池随从** 作为事实上下文交给 DeepSeek，不再任意截前 420/520 张；
7. DeepSeek 输出 CardId 后，Rust 再按同一份 HDT 当前池验证。

因此本版原则是：

> HDT = facts；DeepSeek = reasoning；Rust = validation + execution。

### 2. 推荐卡牌没有框选

V0.2.x 只有在：

```text
Recruit + 当前商店非空 + Watchlist 真正命中
```

时才画框。因此“AI 没有推荐到当前商店”与“Overlay 画坏了”在视觉上完全一样。

V0.3 在游戏右下角固定增加诊断：

```text
商店 6 · 推荐 8 · 命中 2
HDT池：137 张
```

并增加：

```text
[ 测试框选：框第1/3张 ]
```

测试框完全不依赖 DeepSeek 和 Watchlist。购买阶段点击后，第 1/3 个卡槽会直接出现 S/A 镂空测试框。

## 推荐的首次运行顺序

```powershell
cd E:\UserData\Desktop\hearthcoach_deepseek_demo_v0_3

cargo test
cargo run -- carddb rebuild
cargo run -- carddb status
cargo run --bin hearthcoach_demo
```

`carddb status` 应该能看到类似：

```text
HDT HearthDb.dll : ...\HearthDb.dll
HDT CardDefs      : ...\CardDefs.base.xml
Rust cache        : ...\hearthdb_cards_v4_zhCN.json
Cards             : ...
Bacon pool minions: > 0
```

如果 `Bacon pool minions` 是 0，不要开始 AI 测试，先检查 HDT / CardDefs。

## API Key

`hearthcoach_demo.json`：

```json
{
  "deepseek": {
    "base_url": "https://api.deepseek.com",
    "api_key": "你的 Key",
    "model": "deepseek-v4-flash",
    "thinking": false,
    "max_tokens": 4096
  }
}
```

API Key 只保存在本地配置文件，该文件已被 `.gitignore` 忽略。

## 游戏内验收

进入酒馆战棋 Recruit 阶段后先看右下角：

```text
商店 N · 推荐 0 · 命中 0
HDT池：N 张
```

### A. 先独立测试 Overlay

点击：

```text
测试框选：框第1/3张
```

期望：

- 第 1 张卡出现红色镂空 S 框；
- 第 3 张卡出现黄色镂空 A 框；
- 中间完全透明；
- Combat 阶段立即隐藏；
- 再点按钮可关闭。

如果这一步不工作，问题就是 Overlay/坐标，不需要碰 DeepSeek。

### B. 再测试 HDT Knowledge + AI

点击“AI 分析可玩阵容”，选择一个阵容。面板生成前/中/后期 Watchlist 后，右下角的：

```text
推荐 N
```

应该大于 0。

刷商店时：

```text
命中 N
```

只要大于 0，正式推荐框就必须出现。如果 `命中=0`，这是当前商店没有 Watchlist 卡，不是 Overlay 失效。

## 当前 Watchlist 只推荐随从

为了彻底解决“旧知识”和卡池合法性，本 Demo V0.3 的 Watchlist 只从当前 `BaconPoolMinions` 选择可购买随从。酒馆法术、饰品等以后可以增加各自的 HDT/current-pool provider；本版不让 DeepSeek 凭记忆补这些事实。

## Overlay 配置

仍可通过 `hearthcoach_demo.json` 校准：

```json
"overlay": {
  "enabled": true,
  "shop_center_x_ratio": 0.505,
  "shop_top_ratio": 0.285,
  "shop_slot_spacing_ratio": 0.079,
  "card_width_ratio": 0.095,
  "card_height_ratio": 0.215,
  "border_px": 4,
  "highlight_delay_ms": 320,
  "panel_width_ratio": 0.27,
  "panel_height_ratio": 0.56,
  "panel_right_margin_ratio": 0.012,
  "panel_bottom_margin_ratio": 0.055,
  "panel_alpha": 235
}
```

测试框模式就是为了让这些坐标可以在不依赖 AI 的情况下校准。

## V0.4 Dynamic Round Planner

V0.4 在 V0.3 基础上加入 Combat→Recruit 小目标规划、Rust Tactical/Action 层、重大事件 Replan、饰品作用优先级预规划，以及 phase/shop 和稳定阵容边界。详细见 `V0_4_IMPLEMENTATION.md`。


## V0.4.1 Compile Fix

- 修复 `src/demo/deepseek.rs` 中 `Option<&CompositionOption>` / `Option<&WatchlistResponse>` / `Option<&RoundPlan>` 传给 `serde_json::to_string` 的类型错误。
- 移除 Overlay 未使用的 `SetTextColor` import。
- `VERIFY_V0_4_1.ps1` 会先自动 `cargo fmt`，随后执行格式检查、`cargo check`、`cargo test`，并对每个 cargo 命令显式检查 `$LASTEXITCODE`；任何一步失败都会立即停止，不会再误报 passed。


## V0.4.2 Correctness Patch

本版集中修复实战暴露的四类问题：

1. **低本牌时效 / 升本**：Rust 使用回合酒馆曲线给卡牌增加 `tier_relevance`。例如 Round 5 的参考酒馆为 T3，T1 卡时效约 0.50、T3 为 1.00；旧 `early` Watchlist 的 S/A 奖励还会再次衰减。升本改为 `prefer / neutral / delay` 软倾向，并加入落后曲线、升本费用、生命风险、当前商店机会成本；`gold_reserve` 不再硬删除升本动作。
2. **饰品识别**：`ChoiceOpened` 后 Source / Option 每次到达都会发 `ChoiceUpdated` 并重新分类，优先通过 Source / Option 的 CardId、CardType、名称识别 Trinket，同时区分 DarkGift / 普通 Discover。真实饰品选择时点可以覆盖预估回合。
3. **框选正确性**：商店不再把 N 张牌拉伸到固定总 span，而使用固定卡槽中心间距 `shop_slot_spacing_ratio=0.079`；每个 `ShopRevision` 原子清除旧 decision hits，只有 `decision_revision == shop_revision` 且视觉 settle (`highlight_delay_ms`, 默认 320ms) 后才允许画框。
4. **Panel V2**：游戏内面板拆成 `当前决策 / 阵容指南` 两页，并适当加宽/增高；动态页展示小目标、升本倾向、前三动作、路线、饰品预案和稳定阵容，Guide 页保留原有前/中/后期 Watchlist。

Windows 验证：

```powershell
powershell -ExecutionPolicy Bypass -File .\VERIFY_V0_4_2.ps1
```


## V0.4.3 compact overlay

- compact bottom-right decision card
- horizontal top composition guide with best-effort local card art
- Trinket choice temporarily replaces tactical action list
- increased overlay opacity
- strategic JSON missing target fields use Rust defaults


## V0.4.4 UI controls

- 顶部阵容指南会根据当前回合自动切换前期 / 中期 / 后期。
- 按住右下角 `HearthCoach` 标题栏即可拖动决策框。
- 拖动右下角 `↘` 可自由缩放。
- 布局自动保存到 `hearthcoach_demo.json`；点击标题栏右侧 `复位` 恢复默认布局。


## V0.4.5

详见 `V0_4_5_INTERACTION_CHAT.md`。

## V0.4.6 real-match fixes

- **Concrete Trinket names**: an open Trinket Choice exposes its real candidate names immediately. After a short debounce, DeepSeek ranks only those concrete candidates against the current RoundPlan/TrinketPlan. Invalid or missing model output falls back locally without losing the names.
- **Guide art correctness**: card art lookup is restricted to explicit card-art folders / trusted HDT image folders, requires safe CardId filename matching and rejects implausible sprite aspect ratios. Drawing uses center-crop instead of stretch; unreliable art falls back to the text tile.
- **Pixel wrapping**: compact decision text uses `GetTextExtentPoint32W` so resized narrow panels wrap by real rendered width.
- **Stable chat input**: chat is a focusable non-layered Win32 popup with a native child `EDIT`. `WS_CLIPCHILDREN` and chat-state-based repainting prevent the parent overlay from painting over text while the user types.

Windows 验证：

```powershell
powershell -ExecutionPolicy Bypass -File .\VERIFY_V0_4_6.ps1
```


## V0.4.7

Grounded chat + compact panel text correctness。运行 `VERIFY_V0_4_7.ps1` 后再实战。


## V0.5.0.7 Power.log watcher correctness

当炉石客户端重启、崩溃或直接退出而旧日志未写 `STATE=COMPLETE` 时，HearthCoach 不再被旧 runtime 的 active 状态锁死。磁盘出现新的 Power.log 会立即触发 source rotation。环境诊断页新增“刷新日志”按钮，可手动强制重扫。
