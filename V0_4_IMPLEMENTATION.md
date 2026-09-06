# HearthCoach Demo V0.4.1 — Dynamic Round Planner

本版基于 V0.3 的 HDT Knowledge + Overlay Diagnostics，新增三层决策和稳定状态边界。

## 1. 决策节奏

- **Combat / Strategic Planner (DeepSeek)**：Combat 开始立即为下一 Recruit 生成 `RoundPlan`。
- **Recruit / Tactical Planner (Rust)**：每个稳定 `ShopUpdated` 根据 `RoundPlan` 生成短行动路线。
- **Recruit / Action Evaluator (Rust)**：购买、打出、升级、刷新、冻结等确定动作实时评分。
- **Major Replan**：酒馆升级、核心牌到手、阵容方向改变、饰品完成、生命跨危险阈值等会让慢计划失效并重算；普通刷新不会调用 LLM。

## 2. 饰品预规划

`agent.trinket_rounds` 默认 `[6, 9]`，所以第 5/8 回合 Combat 会提前生成下一回合的 `TrinketPlan`：

- `desired_effect`：希望饰品解决什么问题；
- `role_priorities`：即时战力 / 生存 / 体系协同 / 长期成长 / 经济等作用的 0~10 优先级；
- `avoid_roles`：当前局势不希望拿的作用；
- 不猜具体饰品，真实 `ChoiceKind::Trinket` 始终是权威事件。

特殊规则改变饰品时点时，可改 `trinket_rounds`；即使没有提前预测到，Harness 检测到真实 Trinket Choice 后也会立即补规划。

## 3. Overlay / 阵容稳定性

- `set_shop_revision()` 不再修改 phase。
- 只有 `current_phase == Recruit && current_round == ShopUpdated.round` 才接收商店更新。
- `phase_epoch` 在阶段切换时递增；异步战略规划另有 `planner_generation` fence，旧结果不能覆盖新计划。
- `stable_board` 只在稳定 Recruit snapshot 上提交；Combat 保持上一 Recruit ownership board，不消费战斗复制体/召唤/死亡变化。
- 框选优先显示本地动态 Action Evaluator 的购买建议；没有动态建议时回退到 HDT Watchlist。

## 4. 新增核心文件/接口

- `src/demo/agent.rs`
  - `AgentSnapshot`
  - `DecisionFingerprint`
  - `detect_replan()`
  - `evaluate_recruit()`
  - `fallback_strategic_plan()`
- `src/demo/model.rs`
  - `RoundPlan`
  - `TrinketPlan`
  - `DecisionWeights`
  - `ReplanEvent / ReplanLevel`
  - `TacticalPlan / ActionRecommendation`
- `src/harness/monitor.rs`
  - `watch_one_match_with_catalog_and_tribes_state()`，事件回调同时拿到 `&HarnessRuntime`。
- `src/demo/server.rs`
  - `observe_runtime()` 是 Harness → Agent 的唯一桥接入口。

## 5. 本机验证

PowerShell：

```powershell
cargo fmt
cargo fmt -- --check
cargo check --bin hearthcoach_demo
cargo test
cargo run --bin hearthcoach_demo
```

也可以直接运行：

```powershell
powershell -ExecutionPolicy Bypass -File .\VERIFY_V0_4_1.ps1
```

### V0.4.1 compile fix

根据 Windows `cargo check` 实测，修正了 `plan_next_recruit()` 中三个 `Option<&T>` 的 JSON 序列化调用：必须把 `Option` 本身按引用传给 `serde_json::to_string`，这样 `Some` 正常序列化、`None` 自动输出 `null`。同时移除了 Overlay 的未使用 import。

验证脚本也改成显式检查 `$LASTEXITCODE`；Windows PowerShell 5.1 的 `$ErrorActionPreference = "Stop"` 本身不会因为原生命令返回非零退出码而终止，所以旧脚本会错误打印 `verification passed`。V0.4.1 不再有这个问题。
