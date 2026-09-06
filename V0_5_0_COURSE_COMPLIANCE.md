# HearthCoach V0.5.0 — HDT-style Control Center + Course Compliance

V0.5.0 does **not** replace the in-game assistant with a giant settings UI.
It splits the product into two surfaces backed by the same Rust state:

```text
                      HearthCoach Rust Core
             ┌─────────────┼──────────────┐
             │             │              │
          Harness        Agent       Compliance Core
             │             │        Task/Session/Usage
             └─────────────┼──────────────┘
                           │
              ┌────────────┴────────────┐
              │                         │
     Desktop Control Center       In-game Overlay
     settings/history/audit       live coaching only
```

Run the new application with:

```powershell
cargo run --bin hearthcoach
```

The legacy debug/demo binary is still present:

```powershell
cargo run --bin hearthcoach_demo
```

## Desktop Control Center

The native Windows program is a normal desktop window (not topmost and not drawn over Hearthstone). It can be minimized while playing. The game still uses the existing lightweight overlay.

Pages:

1. **首页** — connection state, round/phase, live Agent status, session id, current token/cost totals, live long-task progress, recent auditable Agent trace.
2. **模型配置** — API compatibility mode, Endpoint, API Key, model, context window, output limit, thinking mode, timeout, per-1M input/output/cache price, token budget and cost budget. `测试连接` saves the edited values and runs a cancellable API task.
3. **历史对局 / AI轨迹** — completed session list, final placement, selected composition, token/cost; full structured RoundPlan/TrinketPlan/TacticalPlan, task history, chat, API records and Agent trace can be viewed. A stored session can be loaded for audit, or its Agent-side context can be restored when no live match is active.
4. **Token / 成本** — exact provider `response.usage` input/output/total/cache token values, input/cache/output cost breakdown and every individual API call.
5. **实时任务** — stage, percentage, elapsed time, details and status. Selected or all running tasks can be cancelled.

## R1 — Core logic in Rust

The main flow is still Rust:

```text
Power.log → parser → EntityStore → StateProjector → HarnessRuntime
         → Agent RoundPlan/Replan → local Tactical Evaluator
         → API orchestration → validation → Overlay / Control Center
```

The new TaskManager, Session archive, usage/cost accounting, budget checks, provider calls and Windows desktop UI are also Rust.

## R2 — User interface

Two UIs are provided:

- native desktop Control Center;
- existing native in-game overlay.

Both consume the same `DemoState` / Rust core.

## R3 — User-configurable model

`DeepSeekConfig` now contains:

```text
api_compatibility = deepseek | openai
base_url
api_key
model
context_window
max_tokens
thinking
timeout_seconds
pricing.input_per_million
pricing.output_per_million
pricing.cached_input_per_million
budget.max_total_tokens
budget.max_cost
```

`openai` compatibility mode omits DeepSeek-specific request fields so OpenAI-compatible local/proxy endpoints can be used. The configured context window is enforced as an Agent pre-flight prompt budget. The desktop UI edits all of these fields.

## R4 — Live progress and cancellation

All potentially slow LLM operations are registered in the Rust `TaskManager`, including:

- composition analysis;
- watchlist generation;
- combat strategic planning / replan;
- concrete Trinket ranking;
- AI chat;
- model connection test.

Each task exposes:

```text
task_id / kind / round
stage / progress_percent / detail
started_at / finished_at / status
```

The Control Center refreshes every 500 ms and displays elapsed time while a task is running.

Cancellation is not "ignore the late response": each task owns an `AtomicBool` cancellation token. `reqwest` request-send and response-body futures are polled with `tokio::select!`; cancellation drops the active HTTP future. Generation fences continue to prevent stale responses from overwriting newer game state.

## R5 — Complete session/history management

Every completed match can automatically write:

```text
hearthcoach_sessions/session_<timestamp>.json
```

`AgentSessionArchive` contains:

- complete `GameArchive` (round/recruit/combat/actions/choices/lobby);
- model configuration snapshot **without the API secret**;
- pricing/budget snapshot;
- available tribes and selected composition;
- Watchlist;
- latest structured RoundPlan / TrinketPlan / TacticalPlan;
- ReplanEvents;
- chat history;
- every API call and exact usage/cost;
- Task history;
- chronological auditable Agent Trace.

Strategic planner traces store the structured plan output, not private chain-of-thought. Tactical traces store ranked actions; API traces store usage/cost/status. Sessions can be saved manually, viewed later, loaded, and Agent-side context can be restored in audit mode without pretending a historical Harness is a live game.

## R6 — Exact token/cost accounting and hard budget

For every physical API request, the provider response `usage` is recorded separately:

```text
prompt_tokens
completion_tokens
total_tokens
prompt_cache_hit_tokens (DeepSeek)
prompt_tokens_details.cached_tokens (OpenAI-compatible, when provided)
```

Cost is calculated using the price configured at the time the response is observed:

```text
uncached_input × input_price
cached_input   × cached_input_price
output         × output_price
```

The UI displays per-call and aggregate usage/cost. Token and currency budgets are configurable. New LLM tasks are rejected once the budget is exhausted, and when an in-flight API response crosses the limit, other running LLM tasks receive cancellation immediately so no further request/retry is started.

## Verification

On the user's Windows machine:

```powershell
powershell -ExecutionPolicy Bypass -File .\VERIFY_V0_5_0.ps1
```

Then start the HDT-style app:

```powershell
cargo run --bin hearthcoach
```

Recommended manual acceptance:

1. Control Center opens; Hearthstone has only the old lightweight Overlay.
2. Model page saves Endpoint/API Key/context/pricing/budget and connection test appears in the Task page.
3. During a >3 s request the Task page keeps updating elapsed time; Cancel changes the task to CancelRequested/Cancelled and the request stops.
4. Play one full match; a JSON session appears under `hearthcoach_sessions` and History can open it.
5. The history detail includes GameArchive-backed match facts plus planner/tactical/task/chat/API trace.
6. Token/Cost page matches provider-reported input/output token usage.
7. Set a very small token budget; after it is crossed, remaining tasks are automatically cancelled and new LLM tasks are refused until the budget is increased.
