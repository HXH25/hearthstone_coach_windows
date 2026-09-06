# HearthCoach V0.4.5 — Hover Details + AI Dialogue

## 1. 阵容指南悬浮说明

顶部自动阶段指南中的每张卡现在都有 hover hit-test。悬浮时在指南窗口内部覆盖一个高不透明度 tooltip，显示：

- S/A/B
- 酒馆等级
- 卡名
- `role`
- `reason`

不需要点击，也不会改变自动阶段。

## 2. 小目标折叠

决策框默认只显示：

```text
▸ 小目标：提高体系协同并为成长留资源
```

鼠标停在这一行时临时显示完整浮层，包含 primary/secondary goal、升本策略与 Replan 条件。浮层覆盖在当前动作上方，移开即恢复，因此不会为了偶尔查看解释而永久放大面板。

## 3. AI 对话 / 修正计划

点击右下角 `AI交流` 会打开一个独立、可获得键盘焦点的 Win32 chat popup。普通 Overlay 仍保持 no-activate；只有玩家主动打开聊天时才暂时取得焦点，因此标准 Windows EDIT/IME 可以输入中文。

对话分两种：

- **讨论**：`plan_patch=null`，只回答问题。
- **明确修正策略**：DeepSeek 返回部分 `RoundPlanPatch`，例如修改 primary goal、direction、升本 posture、刷新预算或决策权重。

Rust 不允许 chat 修改 `target_round`。数值会在本地 clamp，且用户修改计划后会递增 `planner_generation`，使尚未返回的旧慢规划失效。修改成功后立即运行本地 Tactical Evaluator。

示例：

```text
我觉得现在很安全，这回合优先升本，不要继续刷低本牌。
```

模型可以返回类似：

```json
{
  "reply": "可以，当前血量允许把资源转向升本……",
  "plan_patch": {
    "primary_goal": "优先升本，同时只保留高价值体系核心",
    "tier_policy": {"posture":"prefer","target_tier":5},
    "economy": {"max_rerolls":1},
    "weights": {"scaling":2.0,"economy":1.4,"tempo":0.8}
  },
  "patch_summary": "玩家要求本回合优先升本"
}
```

浏览器控制页也暴露同一 `/api/chat` 能力。
