# V0.3 Review & Test

## Scope

只修两类问题：

1. DeepSeek 使用旧知识导致阵容/卡表不可靠；
2. 推荐框未出现时无法诊断 AI 匹配还是 Overlay 绘制。

不修改已经实战验证的 Power parser / EntityStore / Recruit/Combat archive 主干。

## Knowledge changes

### Card cache V4

`CardMeta` 新增：

```rust
in_bacon_pool: bool
```

PowerShell exporter 在加载 HDT 最新 `CardDefs.base.xml` 后直接查询：

```text
HearthDb.Cards.BaconPoolMinions
```

新 cache 文件：

```text
%LOCALAPPDATA%\HearthCoach\hearthdb_cards_v4_zhCN.json
```

### HdtKnowledgeBase

新增 `src/demo/hdt_knowledge.rs`：

- 只接受 `in_bacon_pool=true` 的普通 MINION；
- 按本局 `AvailableTribes` 过滤；
- 中立随从保留；
- 不任意截断当前池；
- 提供 DeepSeek factual prompt context；
- 对 DeepSeek 返回 CardId 再验证并 canonicalize。

Composition 现在必须返回至少两个当前池 `core_card_ids` 作为事实证据；不合法的 Composition 会被丢弃。

Watchlist 每个 early/mid/late 阶段在 HDT 验证后都不能为空，否则请求显式失败，而不是静默生成 0 推荐。

## Overlay diagnostics

`PublicState` 新增：

- `knowledge`
- `current_pool_count`
- `current_stage_recommendation_count`
- `overlay_test_mode`

右下角显示：

```text
商店 N · 推荐 N · 命中 N
HDT池：N 张
```

`overlay_test_mode` 在 Recruit + shop 非空时绕过 `shop_hits`，直接画第 1/3 个槽位。

## Regression tests

新增/更新：

- `catalog_preserves_hdt_bacon_pool_membership`
- `hdt_knowledge_only_exposes_current_pool_and_available_tribes`
- `hdt_knowledge_prompt_is_not_arbitrarily_truncated`
- `overlay_test_mode_can_be_toggled_independently_from_ai_hits`

已有 Recruit/Combat phase test 继续保留。

## Windows compile gate

```powershell
cargo test
```

然后强制重建新知识 cache：

```powershell
cargo run -- carddb rebuild
cargo run -- carddb status
```

必须确认：

```text
Bacon pool minions: 非 0
```

再运行：

```powershell
cargo run --bin hearthcoach_demo
```

## 实战 P0

- [ ] `cargo test` 全通过。
- [ ] `carddb status` 显示 v4 cache，Bacon pool minions > 0。
- [ ] Recruit 面板显示 `商店/推荐/命中/HDT池` 数量。
- [ ] “测试框选”能独立框第 1/3 张卡。
- [ ] 测试框内部透明。
- [ ] Combat 测试框/正式框都隐藏。
- [ ] AI 阵容核心 CardId 全来自当前 HDT 池。
- [ ] Watchlist 生成后 `推荐 N > 0`。
- [ ] `命中 N > 0` 时正式框出现。

## 当前边界

- V0.3 的 HDT Knowledge Watchlist 暂时只覆盖当前酒馆随从池；不让模型凭旧记忆生成酒馆法术/饰品事实。
- Overlay 卡槽仍采用 client-rect + 比例定位；测试框模式用于本机校准。
- 没有游戏进程注入，没有自动购买。
- AI 请求仍是后台线程；课程 R4 streaming/cancel 尚未进入 Demo 范围。

## V0.4.2 real-match acceptance

重点观察以下链路，而不是只看是否能启动：

1. Round 5 左右，低本早期 S 牌不应继续无条件压过当前 3 本 A 牌；落后酒馆曲线且生命安全时，`升级酒馆` 应明显进入前列。
2. 饰品 Choice 打开后，面板应从普通选择自动更新为 `饰品选择 · N 个候选`；Source/Option 晚到不能再导致永久误分类。
3. 每次刷新/购买导致新 ShopRevision 时，旧框应立即消失；约 320ms settle 后，只允许 `decision_revision == shop_revision` 的新框出现。
4. 5/6/7 张商店下，卡槽中心应保持固定 pitch；若仍有整体平移，只调 `shop_center_x_ratio`，若间距统一偏大/偏小，只调 `shop_slot_spacing_ratio`。
5. 游戏内 Panel 的 `当前决策` 与 `阵容指南` 可互相切换；核心动态信息不再和完整 Watchlist 挤在同一页。

## V0.4.4 manual UI checks

1. Start in round 4 and verify top guide says `自动阶段 · 前期`; enter round 5 and verify it changes to `中期` without clicking anything; round 9 should change to `后期`.
2. Drag the `HearthCoach` title bar to another part of the game client; release and restart the demo. Position should persist.
3. Drag the `↘` handle in the lower-right corner both larger and smaller; release and restart. Size should persist.
4. Drag/resize against all four Hearthstone client edges; the panel must remain fully inside the client.
5. Click `复位`; panel should return to the compact bottom-right default and remain there after restart.


## V0.4.5 manual checks

1. 鼠标悬浮顶部任意阵容指南卡，确认出现作用/理由浮层，移开立即消失。
2. 小目标平时只有一行；悬浮时展开，移开恢复，且不改变面板尺寸。
3. 点击 `AI交流`，确认文本框可输入中文；普通提问不改变目标。
4. 输入“这回合优先升本，不要继续刷低本牌”，回复应标记 `[已修改目标]`，右下角小目标/动作随后变化。
5. 对话请求未返回前进入新阶段时，旧阶段 plan patch 不应写入新计划。
6. 浏览器 `/api/chat` 与局内对话使用同一 chat history/state。

## V0.4.6 manual checks

1. 打开饰品选择：在 AI 排序返回前就应看见真实饰品名称；排序完成后显示同一批名称的评分/作用/理由。
2. 顶部 Guide：若本机找到可靠卡图，应比例正常且对应 CardId；找不到时必须退回文字卡，不能显示细长纹理/sprite。
3. 将右下角框缩窄，目标/错误/动作/路线不得从右侧直接消失，应按实际像素宽度换行。
4. 打开 `AI交流`，连续输入中英文 10 秒；输入内容和光标必须保持。发送后文本才清空，并显示用户消息/AI回复。
5. 聊天过程中游戏状态持续刷新，编辑框内容不得因为 Overlay refresh 被覆盖。


## V0.4.7 manual checks

- R4 提问“现在要不要升本”，回答不得把当前局面说成 R1。
- 问“当前阵容核心卡是什么”，回答必须基于当前 composition core_card_ids / HDT facts。
- AI 长回答可在聊天历史框中滚动查看。
- 将右下角缩到最小尺寸后，悬浮小目标文字不得横向越界。
