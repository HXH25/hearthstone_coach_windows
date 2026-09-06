//! HearthCoach Harness
//!
//! The library is intentionally split into four layers:
//!
//! 1. `power` / `choice`: parse Hearthstone log facts.
//! 2. `state`: keep the complete entity fact store and project useful snapshots.
//! 3. `runtime`: split a Battlegrounds match into Recruit/Combat phases and archive one record per round.
//! 4. `monitor`: attach to a live `Power.log` and feed the runtime.
//!
//! Higher-level agents should query `HarnessRuntime` / `GameArchive`, not parse `Power.log` themselves.

pub mod harness;

/// Phase-2 demo UI/AI integration.
pub mod demo;
