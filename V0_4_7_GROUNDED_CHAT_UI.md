# HearthCoach V0.4.7 — Grounded Chat + Text Layout Fix

这版针对 V0.4.6 实战暴露的两个问题：

1. 紧凑决策框的小目标悬浮详情在窄窗口中仍会横向/纵向裁切。
2. AI 对话虽然可输入，但回答只显示少量行，而且模型会误读当前回合或凭记忆补充卡牌事实。

## UI

- 决策框最小尺寸提高到 285×190，避免缩到不可读。
- 小目标悬浮时，详情接管整个决策内容区，不再夹在动作列表中展开。
- 小目标详情、次要目标、升本倾向、Replan 条件全部按 GDI 实际像素宽度换行。
- `draw_wrapped_text` 在达到最大行数时显式显示省略号，不再让文字越界。
- AI 聊天历史改为 Windows 原生只读 multiline EDIT：自动换行、垂直滚动、保留完整回答；输入框继续使用独立原生 EDIT。
- Chat 默认尺寸扩大，并自动滚动到最新消息。

## AI grounding

Chat prompt 现在明确区分：

1. CURRENT FACTS（Harness/Rust 权威当前回合、阶段、HP、金币、酒馆等级）
2. TRUSTED CARD FACTS（HDT/HearthDb CardId/名称/等级/文本）
3. CURRENT TACTICAL PLAN（Rust 当前动作排序）
4. RoundPlan / Composition / Watchlist（战略提示，不是卡牌事实源）
5. 历史对话（最低优先级）

每次回答必须返回：

- `observed_round`：必须与 Rust 当前回合一致；
- `cited_card_ids`：具体卡牌事实必须引用可信 CardId；
- `plan_patch`：只有用户明确要求修改策略时才允许出现。

Rust 会拒绝：

- 把当前 R4 说成 R1 之类的 stale-round 回答；
- 引用不在 HDT/snapshot 可信上下文中的 CardId；
- 卡牌事实问题却完全不给 CardId 依据；
- “核心卡”问题绕开当前选中阵容的 `core_card_ids` 随意另造核心。

第一次 grounding 失败会自动让 DeepSeek 重答一次，第二次仍失败才向 UI 报错。

## 验证

```powershell
powershell -ExecutionPolicy Bypass -File .\VERIFY_V0_4_7.ps1
cargo run --bin hearthcoach_demo
```
