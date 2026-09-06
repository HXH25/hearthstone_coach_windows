# V0.4.1 Fix

Windows 实测 V0.4 暴露三个编译错误，均位于 `src/demo/deepseek.rs::plan_next_recruit()`：

```rust
serde_json::to_string(selected)
serde_json::to_string(watchlist)
serde_json::to_string(previous_plan)
```

三个变量的类型分别是 `Option<&CompositionOption>`、`Option<&WatchlistResponse>`、`Option<&RoundPlan>`。`serde_json::to_string` 的参数类型是 `&T`，因此应该序列化 `Option` 本身：

```rust
serde_json::to_string(&selected)
serde_json::to_string(&watchlist)
serde_json::to_string(&previous_plan)
```

这样 `None` 会自然序列化为 JSON `null`，不应该使用 `expect()` 强行解包。

此外，旧 PowerShell 验证脚本只设置 `$ErrorActionPreference = "Stop"`。Windows PowerShell 5.1 对 `cargo` 这类 native executable 的非零退出码不会自动抛异常，因此即使 `cargo check/test` 失败，脚本仍会继续打印 `verification passed`。新脚本逐项检查 `$LASTEXITCODE`。
