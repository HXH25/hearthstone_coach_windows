# HearthCoach V0.5.0.7 — Latest Power.log + Manual Refresh

## 修复的问题

V0.5.0.6 只有在 `!runtime.is_match_active()` 时才允许切换到另一份 Power.log。若 Hearthstone 在一局中直接退出/崩溃，旧日志可能没有 `STATE=COMPLETE`，于是 runtime 永远保持 active；即使下一次启动炉石生成了更新的 Power.log，watcher 仍会被旧文件锁死。

## 新逻辑

每次 poll 都重新递归扫描 `<Hearthstone>/Logs` 下最新的 `Power.log`：

```text
latest Power.log == followed Power.log
    -> 正常继续 tail

latest Power.log != followed Power.log
    -> flush 当前 partial round/shop
    -> HarnessEvent::MatchInterrupted
    -> 封存旧 Agent session
    -> 清空 live state
    -> watcher 返回外层
    -> 重新打开磁盘最新 Power.log
```

这里**不再检查**旧 runtime 是否仍认为 match active。物理日志源更新是更高优先级的事实。

## 手动“刷新日志”

Control Center -> 环境诊断新增：

```text
[重新检测] [自动检测并修复] [刷新日志]
```

“刷新日志”不是普通 UI refresh。它会增加一个跨线程 generation：

```text
UI request_log_refresh()
    -> AtomicU64 generation + 1
    -> 当前 watcher 检测 generation 变化
    -> 安全封存旧 live session
    -> 退出 watcher
    -> 外层重新扫描最新 Power.log
```

历史 Session、Token、API call 和 Agent trace 会保留；当前 round/shop/plan/watchlist 等 live state 会清空，再由最新日志重建。

## 诊断信息

环境页现在同时显示：

- 磁盘最新 `Power.log`
- 当前 watcher 正在监听的 `Power.log`

两者不一致时显示警告，并提示可以点击“刷新日志”。

## 为什么要显式 MatchInterrupted

直接丢弃旧 runtime 会让课程 R5 的轨迹断裂。V0.5.0.7 增加：

```rust
HarnessEvent::MatchInterrupted { reason: String }
```

中断时会把当前 partial `GameArchive`、RoundPlan、聊天、API usage、Task/Trace 一起封存为历史会话，再清除实时状态。
