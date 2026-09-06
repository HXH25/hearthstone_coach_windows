# HearthCoach Demo V0.4.2 — Correctness Patch

This patch is based on the Windows-verified V0.4.1 tree.

## 1. Round-aware card value and leveling

The local Action Evaluator now owns a conservative tavern curve:

```text
R1       -> T1
R2-R3    -> T2
R4-R5    -> T3
R6-R7    -> T4
R8-R9    -> T5
R10+     -> T6
```

`tier_relevance()` reduces the value of cards below the current round/current tavern reference. On Round 5:

```text
T1 relevance ~= 0.50
T2 relevance ~= 0.75
T3 relevance  = 1.00
```

The multiplier applies to the S/A/B Watchlist bonus too. Recommendations that only survive from an older stage also receive `watch_stage_relevance`, so an old early-game S card cannot keep its full S bonus forever.

Upgrade planning is no longer a hard `allow_upgrade` boolean. `TierPolicy` exposes a soft `UpgradePosture::{Prefer, Neutral, Delay}` and Rust combines it with:

- how far the player is behind the round curve;
- current upgrade cost;
- health risk;
- current-shop opportunity cost;
- scaling/economy/tempo weights;
- gold-reserve breach as a penalty rather than a hard ban.

## 2. Trinket choice timing

`ChoiceOpened` is not sufficient to classify a choice because Hearthstone prints the Source and Options later. Harness now emits `ChoiceUpdated { id }` whenever Source or an Option arrives. `current_choice()` is re-materialized after every update.

Classification checks source/option CardId, CardType and source name for Trinket markers, separately recognizes DarkGift, and leaves other GENERAL choices as Discover. An actual Trinket choice can therefore trigger a TrinketPlan even if the configured forecast round did not match.

## 3. Overlay highlight correctness

Highlight geometry now uses a fixed adjacent-slot pitch instead of stretching the row across a total span:

```text
shop_center_x_ratio       = 0.505
shop_slot_spacing_ratio   = 0.079
shop_top_ratio            = 0.285
card_width_ratio          = 0.095
card_height_ratio         = 0.215
```

On every new ShopRevision:

1. old dynamic hits are cleared immediately;
2. `decision_revision` is reset;
3. Tactical evaluation recomputes against the new shop;
4. results are committed only if their revision still matches;
5. the overlay waits `highlight_delay_ms` (default 320 ms) for client animation settle;
6. only matching-revision hits are drawn.

If a TacticalPlan exists but recommends no purchase, the UI no longer falls back to stale/static Watchlist borders.

## 4. Panel V2

The in-game panel is split into:

- **当前决策**: current choice, AI status, RoundPlan, upgrade posture, top actions, route, TrinketPlan, stable board.
- **阵容指南**: composition selection, early/mid/late Watchlist, expandable explanations and paging.

Default panel size is increased to 27% client width x 56% client height while preserving the existing configurable margins/alpha.

## Verify on Windows

```powershell
powershell -ExecutionPolicy Bypass -File .\VERIFY_V0_4_2.ps1
```

Then:

```powershell
cargo run --bin hearthcoach_demo
```
