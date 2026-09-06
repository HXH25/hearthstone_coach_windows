use std::{collections::HashMap, time::{Duration, Instant}};

use serde::{Deserialize, Serialize};

use crate::harness::{
    archive::{
        ChoiceArchive, ChoiceKind, CombatArchive, CombatResult, GameArchive, RecruitActionKind,
        RecruitActionRecord, RecruitArchive, RoundArchive, ShopRevision, ShopRevisionCause,
    },
    card_catalog::{CardCatalog, CardMeta, ResolvedCardMeta},
    choice::ChoiceEvent,
    power::{EntityDescriptor, EntityIdentity, PowerEvent, TagTarget, TagValue},
    router::{LogEvent, LogRouter, ParsedLogEvent},
    state::{CardSnapshot, EntityStore, HeroSnapshot, LiveCombatSnapshot, RecruitSnapshot, StateProjector},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseKind {
    Pregame,
    Recruit,
    Combat,
    Complete,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HarnessEvent {
    MatchStarted,
    /// The physical Power.log source changed or the user forced a rescan before
    /// Hearthstone emitted STATE=COMPLETE. Consumers should archive the partial
    /// session and clear live state before opening the newest log.
    MatchInterrupted { reason: String },
    PhaseStarted { round_number: u32, phase: PhaseKind },
    ChoiceOpened { id: u32 },
    /// Source/options arrive after ChoiceOpened in Power.log. Consumers that
    /// classify choices (especially Trinkets) must listen for these updates.
    ChoiceUpdated { id: u32 },
    ChoiceResolved { id: u32 },
    RecruitAction { round_number: u32, kind: RecruitActionKind },
    ShopUpdated {
        round_number: u32,
        revision: u32,
        card_ids: Vec<String>,
    },
    RoundArchived { round_number: u32 },
    MatchCompleted,
}

#[derive(Debug)]
struct RecruitBuilder {
    round_number: u32,
    raw_game_turn: u32,
    started_at: Option<String>,
    start: Option<RecruitSnapshot>,
    actions: Vec<RecruitActionRecord>,
    shop_revisions: Vec<ShopRevision>,
    last_emitted_shop_signature: Option<Vec<(u32, i32, String)>>,
    last_observed_shop_signature: Option<Vec<(u32, i32, String)>>,
    last_stable_shop: Vec<CardSnapshot>,
    shop_dirty: bool,
    pending_shop_cause: ShopRevisionCause,
    pending_shop_timestamp: Option<String>,
    shop_last_changed_at: Option<Instant>,
    choices: Vec<ChoiceArchive>,
}

#[derive(Debug)]
struct CombatBuilder {
    round_number: u32,
    raw_game_turn: u32,
    started_at: Option<String>,
    start: Option<LiveCombatSnapshot>,
    player_hero_entity_id: Option<u32>,
    opponent_hero_entity_id: Option<u32>,
    player_effective_start: Option<i32>,
    opponent_effective_start: Option<i32>,
    player_effective_last: Option<i32>,
    opponent_effective_last: Option<i32>,
    won_last_combat: Option<bool>,
    explicit_damage_taken: Option<i32>,
    player_predamage_max: Option<i32>,
    opponent_predamage_max: Option<i32>,
    choices: Vec<ChoiceArchive>,
}

#[derive(Debug, Clone)]
struct PendingAction {
    timestamp: Option<String>,
    kind: RecruitActionKind,
    source: Option<CardSnapshot>,
    target: Option<CardSnapshot>,
}

#[derive(Debug)]
struct BlockFrame;

#[derive(Debug, Clone)]
struct PendingUserInput {
    timestamp: Option<String>,
    #[allow(dead_code)]
    selected_option: i32,
    selected_target: Option<u32>,
    #[allow(dead_code)]
    selected_position: Option<i32>,
}

#[derive(Debug, Clone)]
struct PendingChoice {
    id: u32,
    choice_type: String,
    count_min: usize,
    #[allow(dead_code)]
    count_max: usize,
    expected_selected: Option<usize>,
    started_at: Option<String>,
    opened_turn: u32,
    source: Option<EntityDescriptor>,
    raw_source: Option<String>,
    options: Vec<EntityDescriptor>,
    selected: Vec<EntityDescriptor>,
}

/// Main library entry point for future Agent/UI code.
///
/// Feed physical log lines with `feed_line`. Query current data through the
/// getter methods without touching Power.log parsing internals.
#[derive(Debug)]
pub struct HarnessRuntime {
    router: LogRouter,
    store: EntityStore,
    catalog: CardCatalog,
    archive: GameArchive,
    source_log: Option<String>,
    active: bool,
    complete_seen: bool,
    current_raw_turn: Option<u32>,
    current_recruit: Option<RecruitBuilder>,
    current_combat: Option<CombatBuilder>,
    pending_recruit_archive: Option<RecruitArchive>,
    /// Diagnostic Power block nesting only. User action recognition is anchored
    /// to GameState.SendOption and no longer depends on this stack depth.
    block_stack: Vec<BlockFrame>,
    pending_user_input: Option<PendingUserInput>,
    /// A semantic recruit action remains open until the next SendOption or the
    /// phase boundary so `state_after` includes all asynchronous consequences.
    pending_recruit_action: Option<PendingAction>,
    open_choices: HashMap<u32, PendingChoice>,
    timer_anchor: Option<Instant>,
    timer_timeout_seconds: Option<i32>,
    /// Explicit UI state tracked from the freeze button/action stream. The
    /// Power.log FROZEN tags are not consistently present on every shop card.
    shop_frozen_state: Option<bool>,
    /// Diagnostic counter for defensive recovery from unmatched BLOCK_STARTs.
    stale_block_stack_recoveries: u32,
}

impl HarnessRuntime {
    /// Library-friendly constructor. It never launches PowerShell. If a
    /// previously generated HearthDb cache exists, it is used; otherwise the
    /// runtime still works with raw Power.log names/CardIds.
    pub fn new(source_log: Option<String>) -> Self {
        let catalog = CardCatalog::load_cached_default().unwrap_or_default();
        Self::with_catalog(source_log, catalog)
    }

    pub fn with_catalog(source_log: Option<String>, catalog: CardCatalog) -> Self {
        let mut archive = GameArchive::default();
        archive.source_log = source_log.clone();
        archive.card_catalog_source = catalog.source().map(str::to_owned);
        Self {
            router: LogRouter::new(),
            store: EntityStore::new(),
            catalog,
            archive,
            source_log,
            active: false,
            complete_seen: false,
            current_raw_turn: None,
            current_recruit: None,
            current_combat: None,
            pending_recruit_archive: None,
            block_stack: Vec::new(),
            pending_user_input: None,
            pending_recruit_action: None,
            open_choices: HashMap::new(),
            timer_anchor: None,
            timer_timeout_seconds: None,
            shop_frozen_state: None,
            stale_block_stack_recoveries: 0,
        }
    }

    pub fn feed_line(&mut self, line: &str) -> Vec<HarnessEvent> {
        let Some(parsed) = self.router.parse_line(line) else {
            return Vec::new();
        };
        self.feed_event(parsed)
    }

    pub fn entity_store(&self) -> &EntityStore {
        &self.store
    }

    pub fn card_catalog(&self) -> &CardCatalog {
        &self.catalog
    }

    /// Resolve one CardId against HDT/HearthDb static metadata. Premium
    /// Battlegrounds cards transparently fall back to their normal-card metadata
    /// when needed.
    pub fn card(&self, card_id: &str) -> Option<ResolvedCardMeta<'_>> {
        self.catalog.resolve(card_id)
    }

    /// Exact CardId lookup without premium -> normal fallback.
    pub fn exact_card(&self, card_id: &str) -> Option<&CardMeta> {
        self.catalog.get(card_id)
    }

    pub fn archive(&self) -> &GameArchive {
        &self.archive
    }

    pub fn is_match_active(&self) -> bool {
        self.active
    }

    pub fn is_complete_seen(&self) -> bool {
        self.complete_seen
    }

    pub fn current_raw_turn(&self) -> Option<u32> {
        self.current_raw_turn
    }

    pub fn current_round_number(&self) -> Option<u32> {
        self.current_raw_turn.map(round_number)
    }

    pub fn current_phase_kind(&self) -> PhaseKind {
        if self.complete_seen {
            return PhaseKind::Complete;
        }
        match self.current_raw_turn {
            None if self.active => PhaseKind::Pregame,
            Some(turn) if turn % 2 == 1 => PhaseKind::Recruit,
            Some(_) => PhaseKind::Combat,
            None => PhaseKind::Unknown,
        }
    }

    pub fn local_player_id(&self) -> Option<i32> {
        self.store.local_player_id()
    }

    pub fn local_player_name(&self) -> Option<&str> {
        self.store.local_player_name()
    }

    pub fn current_recruit_state(&self) -> Option<RecruitSnapshot> {
        let turn = self.current_raw_turn?;
        if turn % 2 == 0 {
            return None;
        }
        Some(self.recruit_snapshot(round_number(turn), turn, None))
    }

    pub fn current_combat_state(&self) -> Option<LiveCombatSnapshot> {
        let turn = self.current_raw_turn?;
        if turn % 2 == 1 {
            return None;
        }
        Some(self.projector().combat_snapshot(round_number(turn), turn, None))
    }

    pub fn current_hero(&self) -> Option<HeroSnapshot> {
        self.projector().local_hero()
    }

    pub fn current_gold(&self) -> Option<i32> {
        self.projector().current_gold()
    }

    pub fn current_tavern_tier(&self) -> Option<u8> {
        self.projector().tavern_tier()
    }

    pub fn current_hero_power(&self) -> Option<crate::harness::state::HeroPowerSnapshot> {
        self.projector().hero_power()
    }

    pub fn current_refresh_cost(&self) -> Option<i32> {
        self.projector().refresh_cost()
    }

    pub fn current_upgrade_cost(&self) -> Option<i32> {
        self.projector().upgrade_cost()
    }

    pub fn is_shop_frozen(&self) -> Option<bool> {
        self.shop_frozen_state.or_else(|| self.projector().shop_frozen())
    }

    /// Number of times a phase boundary repaired a stale/unbalanced block
    /// stack. This is diagnostic only; a non-zero value no longer poisons the
    /// remainder of the match.
    pub fn stale_block_stack_recoveries(&self) -> u32 {
        self.stale_block_stack_recoveries
    }

    pub fn round_archive(&self, round_number: u32) -> Option<&RoundArchive> {
        self.archive.round(round_number)
    }

    pub fn available_tribes(&self) -> Option<&[String]> {
        self.archive.available_tribes.as_deref()
    }

    pub fn available_tribes_source(&self) -> Option<&str> {
        self.archive.available_tribes_source.as_deref()
    }

    pub fn current_shop(&self) -> Vec<CardSnapshot> {
        self.projector().shop()
    }


    /// Minimal fast-path input for ShopWatcher. This returns only a *stable*
    /// shop revision. While a refresh is still being reconstructed, it returns
    /// an empty vector instead of exposing a partial or stale shop.
    pub fn current_shop_card_ids(&self) -> Vec<String> {
        if !self.shop_is_stable() {
            return Vec::new();
        }
        self.stable_shop()
            .into_iter()
            .filter_map(|card| card.card_id)
            .collect()
    }

    /// Last emitted complete shop revision. Consumers that react to
    /// `HarnessEvent::ShopUpdated` should normally use this rather than raw
    /// EntityStore state.
    pub fn stable_shop(&self) -> Vec<CardSnapshot> {
        if let Some(recruit) = self.current_recruit.as_ref() {
            return recruit
                .shop_revisions
                .last()
                .map(|revision| revision.cards.clone())
                .unwrap_or_default();
        }
        self.archive
            .latest_round()
            .and_then(|round| round.recruit.as_ref())
            .and_then(|recruit| recruit.shop_revisions.last())
            .map(|revision| revision.cards.clone())
            .unwrap_or_default()
    }

    pub fn shop_is_stable(&self) -> bool {
        self.current_recruit
            .as_ref()
            .map(|recruit| !recruit.shop_dirty && !recruit.shop_revisions.is_empty())
            .unwrap_or(false)
    }


    pub fn current_board(&self) -> Vec<CardSnapshot> {
        self.projector().board()
    }

    pub fn current_hand(&self) -> Vec<CardSnapshot> {
        self.projector().hand()
    }

    pub fn current_trinkets(&self) -> Vec<CardSnapshot> {
        self.projector().trinkets()
    }

    pub fn lobby_players(&self) -> Vec<crate::harness::state::LobbyPlayerSnapshot> {
        self.projector().lobby_players()
    }

    pub fn next_opponent_player_id(&self) -> Option<i32> {
        self.projector().next_opponent_player_id()
    }

    pub fn current_choice(&self) -> Option<ChoiceArchive> {
        self.open_choices
            .values()
            .max_by_key(|c| c.id)
            .map(|c| self.materialize_choice(c, None))
    }

    /// Minimal fast-path input for Choice/Trinket selector skills.
    pub fn current_choice_option_ids(&self) -> Vec<String> {
        self.current_choice()
            .map(|choice| {
                choice
                    .options
                    .into_iter()
                    .filter_map(|card| card.card_id)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn current_shop_revision(&self) -> Option<&ShopRevision> {
        self.current_recruit
            .as_ref()
            .and_then(|recruit| recruit.shop_revisions.last())
    }

    pub fn shop_revisions(&self, round_number: u32) -> &[ShopRevision] {
        if let Some(recruit) = self.current_recruit.as_ref() {
            if recruit.round_number == round_number {
                return &recruit.shop_revisions;
            }
        }
        self.archive.shop_revisions(round_number)
    }

    pub fn set_available_tribes(&mut self, tribes: Vec<String>) {
        self.set_available_tribes_with_source(tribes, "injected");
    }

    pub fn set_available_tribes_with_source(
        &mut self,
        mut tribes: Vec<String>,
        source: impl Into<String>,
    ) {
        tribes.sort();
        tribes.dedup();
        self.archive.available_tribes = Some(tribes);
        self.archive.available_tribes_source = Some(source.into());
    }

    /// Initial TIMEOUT supplied by the Hearthstone client for the current
    /// Recruit action window.
    pub fn current_timeout_seconds(&self) -> Option<i32> {
        self.projector().timeout_seconds()
    }

    /// Best-effort live countdown derived from TIMEOUT + a local monotonic
    /// clock. Historical replay should use the archived timeout value instead.
    pub fn remaining_seconds(&self) -> Option<i32> {
        if self.current_raw_turn.map(|turn| turn % 2 == 1) != Some(true) {
            return None;
        }
        let timeout = self.timer_timeout_seconds.or_else(|| self.current_timeout_seconds())?;
        let elapsed = self
            .timer_anchor
            .map(|anchor| anchor.elapsed().as_secs() as i32)
            .unwrap_or(0);
        Some((timeout - elapsed).max(0))
    }

    /// Called by the live file watcher after it reaches the current EOF. Shop
    /// updates are emitted only after the entity set has been quiet long enough
    /// to avoid partial 2/7-card refresh snapshots.
    pub fn on_idle(&mut self) -> Vec<HarnessEvent> {
        let mut events = Vec::new();
        let ready = self
            .current_recruit
            .as_ref()
            .and_then(|recruit| recruit.shop_last_changed_at)
            .map(|instant| instant.elapsed() >= Duration::from_millis(150))
            .unwrap_or(false);
        if ready {
            self.flush_shop_revision(None, &mut events);
        }
        events
    }

    /// Deterministic flush for replay/tests and phase boundaries.
    pub fn flush_pending_shop_revision(&mut self) -> Vec<HarnessEvent> {
        let mut events = Vec::new();
        self.flush_shop_revision(None, &mut events);
        events
    }


    /// Force-finalizes a partial stream. Replay uses this at EOF; live watch
    /// normally reaches STATE=COMPLETE first.
    pub fn finish(&mut self, timestamp: Option<String>) -> Vec<HarnessEvent> {
        let mut events = Vec::new();
        self.finalize_pending_recruit_action(timestamp.clone());
        self.pending_user_input = None;
        self.flush_shop_revision(timestamp.clone(), &mut events);
        if self.active && !self.complete_seen {
            self.finalize_current_phase(timestamp.clone(), &mut events);
            self.push_pending_recruit_as_partial_round(&mut events);
            self.archive.ended_at = timestamp;
        }
        self.refresh_metadata();
        events
    }


    fn projector(&self) -> StateProjector<'_> {
        StateProjector::with_catalog(&self.store, &self.catalog)
    }

    fn feed_event(&mut self, parsed: ParsedLogEvent) -> Vec<HarnessEvent> {
        let timestamp = parsed.timestamp;
        let mut events = Vec::new();
        match parsed.event {
            LogEvent::Power(event) => self.handle_power_event(event, timestamp, &mut events),
            LogEvent::Choice(event) => self.handle_choice_event(event, timestamp, &mut events),
        }
        events
    }

    fn handle_power_event(
        &mut self,
        event: PowerEvent,
        timestamp: Option<String>,
        events: &mut Vec<HarnessEvent>,
    ) {
        if matches!(&event, PowerEvent::CreateGame) {
            self.reset_match(timestamp.clone());
            self.store.apply(&event);
            events.push(HarnessEvent::MatchStarted);
            return;
        }

        // SendOption is the client-side proof that the player initiated a new
        // interaction. Finalize the previous action *before* opening the next
        // one so state_after includes all asynchronous consequences of the
        // prior operation, regardless of Power block nesting.
        if let PowerEvent::UserOptionSent {
            selected_option,
            selected_target,
            selected_position,
        } = &event
        {
            self.handle_user_option_sent(
                *selected_option,
                *selected_target,
                *selected_position,
                timestamp.clone(),
                events,
            );
            self.store.apply(&event);
            self.refresh_metadata();
            return;
        }

        // TURN transitions close the previous logical phase before the new
        // TURN tag is applied. A phase boundary is also the final state_after
        // boundary for the last user action of the Recruit phase.
        if let Some(new_turn) = game_turn_from_event(&event) {
            if self.active {
                self.finalize_pending_recruit_action(timestamp.clone());
                self.pending_user_input = None;
                // Do not publish raw shop state here: Bob may already be
                // removing cards one-by-one for combat cleanup. The last
                // stable revision is preserved by finalize_recruit().
                self.finalize_current_phase(timestamp.clone(), events);
            }
            self.store.apply(&event);
            self.current_raw_turn = Some(new_turn);
            self.start_turn(new_turn, timestamp, events);
            self.refresh_metadata();
            return;
        }

        // Combat snapshot must happen before the first real combat ATTACK
        // mutates either board. Recruit-phase ATTACK is a user action and is
        // classified below only when a SendOption is pending.
        if let PowerEvent::BlockStarted { block_type, .. } = &event {
            if block_type == "ATTACK" && self.current_raw_turn.map(|t| t % 2 == 0).unwrap_or(false) {
                self.ensure_combat_started(timestamp.clone());
            }
        }

        self.observe_combat_tag(&event);

        if let PowerEvent::BlockStarted { .. } = &event {
            self.block_stack.push(BlockFrame);

            // Never infer a player action from block depth alone. Hearthstone
            // keeps semantic PLAY/POWER blocks open across later independent
            // inputs. Only consume a block as a user action when a SendOption
            // is waiting for its semantic realization.
            if self.current_raw_turn.map(|t| t % 2 == 1).unwrap_or(false) {
                if let Some(input) = self.pending_user_input.clone() {
                    if let Some(action) = self.detect_recruit_action(
                        &event,
                        input.timestamp.clone().or(timestamp.clone()),
                        input.selected_target,
                    ) {
                        // Defensive only: a new recognized action should always
                        // have been preceded by SendOption, which finalized the
                        // prior one. If not, keep the archive lossless.
                        self.finalize_pending_recruit_action(timestamp.clone());

                        self.flush_shop_revision(timestamp.clone(), events);
                        self.try_start_recruit(timestamp.clone(), true);

                        let kind = action.kind.clone();
                        if let Some(cause) = shop_cause_for_action(&kind) {
                            self.mark_shop_dirty(timestamp.clone(), cause);
                        }
                        self.apply_recruit_action_state(Some(&kind));
                        self.pending_user_input = None;
                        self.pending_recruit_action = Some(action);

                        if let Some(recruit) = self.current_recruit.as_ref() {
                            events.push(HarnessEvent::RecruitAction {
                                round_number: recruit.round_number,
                                kind,
                            });
                        }
                    }
                }
            }
        }

        self.store.apply(&event);
        self.sync_timer_from_state(&event);

        if self.current_raw_turn.map(|turn| turn % 2 == 1) == Some(true) {
            self.bootstrap_shop_frozen_state();
            self.observe_shop_change(timestamp.clone());
            self.try_start_recruit(timestamp.clone(), false);
        }

        if let PowerEvent::TagChanged {
            target: TagTarget::GameEntity,
            tag,
            value,
        } = &event
        {
            if tag == "STEP" && value.as_str() == Some("MAIN_ACTION") {
                // MAIN_ACTION is a RecruitReady boundary, not a block-stack
                // reset. The raw log proves legitimate parent blocks can span
                // this point.
                self.bootstrap_shop_frozen_state();
                self.try_start_recruit(timestamp.clone(), false);
                self.mark_shop_dirty(timestamp.clone(), ShopRevisionCause::PhaseStart);
            }
            if tag == "STEP" && value.as_str() == Some("MAIN_END") {
                self.finalize_pending_recruit_action(timestamp.clone());
            }
        }

        self.refresh_combat_health_tracking();

        if matches!(&event, PowerEvent::BlockEnded) {
            self.block_stack.pop();
        }

        let complete = matches!(
            &event,
            PowerEvent::TagChanged {
                target: TagTarget::GameEntity,
                tag,
                value: TagValue::Symbol(value),
            } if tag == "STATE" && value == "COMPLETE"
        );

        self.refresh_metadata();

        if complete && self.active && !self.complete_seen {
            self.finalize_pending_recruit_action(timestamp.clone());
            self.pending_user_input = None;
            self.flush_shop_revision(timestamp.clone(), events);
            self.finalize_current_phase(timestamp.clone(), events);
            self.push_pending_recruit_as_partial_round(events);
            self.complete_seen = true;
            self.archive.ended_at = timestamp;
            events.push(HarnessEvent::MatchCompleted);
        }
    }


    fn handle_choice_event(
        &mut self,
        event: ChoiceEvent,
        timestamp: Option<String>,
        events: &mut Vec<HarnessEvent>,
    ) {
        if !self.active {
            return;
        }

        match event {
            ChoiceEvent::Opened {
                id,
                player_name: _,
                choice_type,
                count_min,
                count_max,
            } => {
                self.open_choices.insert(
                    id,
                    PendingChoice {
                        id,
                        choice_type,
                        count_min,
                        count_max,
                        expected_selected: None,
                        started_at: timestamp,
                        opened_turn: self.current_raw_turn.unwrap_or(0),
                        source: None,
                        raw_source: None,
                        options: Vec::new(),
                        selected: Vec::new(),
                    },
                );
                events.push(HarnessEvent::ChoiceOpened { id });
            }
            ChoiceEvent::Source { id, source, raw_source } => {
                if let Some(choice) = self.open_choices.get_mut(&id) {
                    choice.source = source;
                    choice.raw_source = Some(raw_source);
                    events.push(HarnessEvent::ChoiceUpdated { id });
                }
            }
            ChoiceEvent::Option { id, index: _, entity } => {
                if let Some(choice) = self.open_choices.get_mut(&id) {
                    choice.options.push(entity);
                    events.push(HarnessEvent::ChoiceUpdated { id });
                }
            }
            ChoiceEvent::SelectionStarted { id, entities_count } => {
                if let Some(choice) = self.open_choices.get_mut(&id) {
                    choice.expected_selected = Some(entities_count);
                }
                if entities_count == 0 {
                    self.resolve_choice(id, timestamp, events);
                }
            }
            ChoiceEvent::Selected { id, index: _, entity } => {
                let should_resolve = if let Some(choice) = self.open_choices.get_mut(&id) {
                    choice.selected.push(entity);
                    let expected = choice.expected_selected.unwrap_or(choice.count_min.max(1));
                    choice.selected.len() >= expected
                } else {
                    false
                };
                if should_resolve {
                    self.resolve_choice(id, timestamp, events);
                }
            }
        }
    }

    fn resolve_choice(
        &mut self,
        id: u32,
        timestamp: Option<String>,
        events: &mut Vec<HarnessEvent>,
    ) {
        let Some(choice) = self.open_choices.remove(&id) else {
            return;
        };
        let opened_turn = choice.opened_turn;
        let archive = self.materialize_choice(&choice, timestamp);
        self.attach_choice(opened_turn, archive);
        events.push(HarnessEvent::ChoiceResolved { id });
    }

    fn materialize_choice(&self, choice: &PendingChoice, resolved_at: Option<String>) -> ChoiceArchive {
        let options: Vec<CardSnapshot> = choice
            .options
            .iter()
            .map(|d| self.snapshot_from_descriptor(d))
            .collect();
        let selected: Vec<CardSnapshot> = choice
            .selected
            .iter()
            .map(|d| self.snapshot_from_descriptor(d))
            .collect();

        let source_card_id = choice.source.as_ref().and_then(|d| {
            self.store
                .entity(d.identity.id)
                .and_then(|e| e.card_id.clone())
                .or_else(|| d.identity.card_id.clone())
        });
        let source_name = source_card_id
            .as_deref()
            .and_then(|id| self.catalog.resolve(id))
            .and_then(|card| card.preferred_name())
            .map(str::to_owned)
            .or_else(|| {
                choice
                    .source
                    .as_ref()
                    .and_then(|d| trustworthy_name(d.identity.name.as_deref()).map(str::to_owned))
            })
            .or_else(|| choice.raw_source.clone().filter(|s| s != "GameEntity"));

        let kind = classify_choice(
            &choice.choice_type,
            source_card_id.as_deref(),
            source_name.as_deref(),
            &options,
        );
        ChoiceArchive {
            id: choice.id,
            choice_type: choice.choice_type.clone(),
            kind,
            started_at: choice.started_at.clone(),
            resolved_at,
            source_name,
            source_card_id,
            options,
            selected,
        }
    }

    fn snapshot_from_descriptor(&self, descriptor: &EntityDescriptor) -> CardSnapshot {
        if let Some(entity) = self.store.entity(descriptor.identity.id) {
            return CardSnapshot::from_entity_with_catalog(entity, Some(&self.catalog));
        }
        CardSnapshot::from_identity(
            descriptor.identity.id,
            descriptor.identity.card_id.clone(),
            descriptor.identity.name.clone(),
            descriptor.controller,
            descriptor.zone.clone(),
            descriptor.zone_position,
            Some(&self.catalog),
        )
    }

    fn snapshot_from_identity(&self, identity: &EntityIdentity) -> CardSnapshot {
        if let Some(entity) = self.store.entity(identity.id) {
            return CardSnapshot::from_entity_with_catalog(entity, Some(&self.catalog));
        }
        CardSnapshot::from_identity(
            identity.id,
            identity.card_id.clone(),
            identity.name.clone(),
            None,
            None,
            None,
            Some(&self.catalog),
        )
    }

    fn attach_choice(&mut self, opened_turn: u32, choice: ChoiceArchive) {
        if opened_turn == 0 {
            self.archive.pregame_choices.push(choice);
        } else if opened_turn % 2 == 1 {
            if let Some(recruit) = self.current_recruit.as_mut() {
                if recruit.raw_game_turn == opened_turn {
                    recruit.choices.push(choice);
                    return;
                }
            }
            if let Some(recruit) = self.pending_recruit_archive.as_mut() {
                if recruit.raw_game_turn == opened_turn {
                    recruit.choices.push(choice);
                }
            }
        } else if let Some(combat) = self.current_combat.as_mut() {
            if combat.raw_game_turn == opened_turn {
                combat.choices.push(choice);
            }
        }
    }

    fn drain_choices_for_turn(
        &mut self,
        turn: u32,
        resolved_at: Option<String>,
    ) -> Vec<ChoiceArchive> {
        let ids: Vec<u32> = self
            .open_choices
            .iter()
            .filter_map(|(id, choice)| (choice.opened_turn == turn).then_some(*id))
            .collect();
        let mut out = Vec::new();
        for id in ids {
            if let Some(choice) = self.open_choices.remove(&id) {
                out.push(self.materialize_choice(&choice, resolved_at.clone()));
            }
        }
        out
    }

    fn reset_match(&mut self, timestamp: Option<String>) {
        self.router.reset();
        self.store.clear();
        self.archive = GameArchive::default();
        self.archive.source_log = self.source_log.clone();
        self.archive.card_catalog_source = self.catalog.source().map(str::to_owned);
        self.archive.started_at = timestamp;
        self.active = true;
        self.complete_seen = false;
        self.current_raw_turn = None;
        self.current_recruit = None;
        self.current_combat = None;
        self.pending_recruit_archive = None;
        self.block_stack.clear();
        self.pending_user_input = None;
        self.pending_recruit_action = None;
        self.open_choices.clear();
        self.timer_anchor = None;
        self.timer_timeout_seconds = None;
        self.shop_frozen_state = None;
        self.stale_block_stack_recoveries = 0;
    }


    fn start_turn(&mut self, raw_turn: u32, timestamp: Option<String>, events: &mut Vec<HarnessEvent>) {
        // A phase/turn boundary is authoritative. Keep Power block nesting as
        // diagnostics only and clear any unmatched frames here. Action
        // recognition itself is independent of this stack.
        self.recover_block_stack();
        let round = round_number(raw_turn);
        if raw_turn % 2 == 1 {
            self.current_recruit = Some(RecruitBuilder {
                round_number: round,
                raw_game_turn: raw_turn,
                started_at: timestamp,
                start: None,
                actions: Vec::new(),
                shop_revisions: Vec::new(),
                last_emitted_shop_signature: None,
                last_observed_shop_signature: None,
                last_stable_shop: Vec::new(),
                shop_dirty: false,
                pending_shop_cause: ShopRevisionCause::PhaseStart,
                pending_shop_timestamp: None,
                shop_last_changed_at: None,
                choices: Vec::new(),
            });
            self.timer_anchor = None;
            self.timer_timeout_seconds = None;
            events.push(HarnessEvent::PhaseStarted {
                round_number: round,
                phase: PhaseKind::Recruit,
            });
        } else {
            self.current_combat = Some(CombatBuilder {
                round_number: round,
                raw_game_turn: raw_turn,
                started_at: timestamp,
                start: None,
                player_hero_entity_id: None,
                opponent_hero_entity_id: None,
                player_effective_start: None,
                opponent_effective_start: None,
                player_effective_last: None,
                opponent_effective_last: None,
                won_last_combat: None,
                explicit_damage_taken: None,
                player_predamage_max: None,
                opponent_predamage_max: None,
                choices: Vec::new(),
            });
            self.timer_anchor = None;
            self.timer_timeout_seconds = None;
            events.push(HarnessEvent::PhaseStarted {
                round_number: round,
                phase: PhaseKind::Combat,
            });
        }
    }


    fn try_start_recruit(&mut self, timestamp: Option<String>, force: bool) {
        let Some(turn) = self.current_raw_turn else { return; };
        if turn % 2 == 0 { return; }
        if self.current_recruit.as_ref().map(|r| r.start.is_some()).unwrap_or(false) {
            return;
        }
        if !force && !self.recruit_resources_ready() {
            return;
        }
        if force && self.projector().current_gold().is_none() {
            return;
        }

        self.bootstrap_shop_frozen_state();
        let mut snapshot = self.recruit_snapshot(round_number(turn), turn, timestamp);
        if snapshot.shop.is_empty() {
            if let Some(stable) = self.current_recruit.as_ref().and_then(|r| r.shop_revisions.last()) {
                snapshot.shop = stable.cards.clone();
            }
        }
        if let Some(recruit) = self.current_recruit.as_mut() {
            recruit.start = Some(snapshot);
        }
        self.sync_timer_anchor();
    }

    fn recruit_resources_ready(&self) -> bool {
        let Some(player_id) = self.store.local_player_id() else { return false; };
        let resources = self.store.player_tag(player_id, "RESOURCES").and_then(TagValue::as_i64);
        let used = self.store.player_tag(player_id, "RESOURCES_USED").and_then(TagValue::as_i64).unwrap_or(0);
        resources.unwrap_or(0) > 0 && used == 0
    }

    fn observe_shop_change(&mut self, timestamp: Option<String>) {
        if self.current_raw_turn.map(|turn| turn % 2 == 1) != Some(true) {
            return;
        }
        let shop = self.projector().shop();
        let signature = shop_signature(&shop);
        let mut changed = false;
        if let Some(recruit) = self.current_recruit.as_mut() {
            if recruit.last_observed_shop_signature.as_ref() != Some(&signature) {
                recruit.last_observed_shop_signature = Some(signature);
                recruit.shop_dirty = true;
                recruit.pending_shop_timestamp = timestamp;
                recruit.shop_last_changed_at = Some(Instant::now());
                changed = true;
            }
        }
        if changed {
            self.try_start_recruit(None, false);
        }
    }

    fn mark_shop_dirty(&mut self, timestamp: Option<String>, cause: ShopRevisionCause) {
        if let Some(recruit) = self.current_recruit.as_mut() {
            recruit.shop_dirty = true;
            recruit.pending_shop_cause = cause;
            if timestamp.is_some() {
                recruit.pending_shop_timestamp = timestamp;
            }
            recruit.shop_last_changed_at = Some(Instant::now());
        }
    }

    fn flush_shop_revision(
        &mut self,
        timestamp: Option<String>,
        events: &mut Vec<HarnessEvent>,
    ) {
        let Some(turn) = self.current_raw_turn else { return; };
        if turn % 2 == 0 { return; }
        if !self
            .current_recruit
            .as_ref()
            .map(|recruit| recruit.shop_dirty)
            .unwrap_or(false)
        {
            return;
        }

        let shop = self.projector().shop();
        // An empty transient shop commonly appears while Bob tears down the old
        // refresh. Never emit that as a stable ShopUpdated event.
        if shop.is_empty() {
            return;
        }
        let signature = shop_signature(&shop);
        let can_refresh_start = self
            .current_recruit
            .as_ref()
            .map(|recruit| {
                recruit.shop_revisions.is_empty()
                    && recruit.actions.is_empty()
                    && (recruit.start.is_some() || self.recruit_resources_ready())
            })
            .unwrap_or(false);
        let refresh_start = can_refresh_start
            .then(|| self.recruit_snapshot(round_number(turn), turn, timestamp.clone()));

        let mut emitted = None;
        if let Some(recruit) = self.current_recruit.as_mut() {
            recruit.last_stable_shop = shop.clone();
            if let Some(snapshot) = refresh_start {
                // The first stable shop is the most reliable RecruitReady point:
                // resources, lobby state, costs and Bob's entities have all settled,
                // and no user action has happened yet.
                recruit.start = Some(snapshot);
            }
            recruit.shop_dirty = false;
            recruit.shop_last_changed_at = None;

            if recruit.start.as_ref().map(|start| start.shop.is_empty()).unwrap_or(false) {
                if let Some(start) = recruit.start.as_mut() {
                    start.shop = shop.clone();
                }
            }

            if recruit.last_emitted_shop_signature.as_ref() == Some(&signature) {
                recruit.pending_shop_timestamp = None;
                recruit.pending_shop_cause = ShopRevisionCause::StateChange;
                return;
            }
            recruit.last_emitted_shop_signature = Some(signature);
            let revision_number = recruit.shop_revisions.len() as u32 + 1;
            let revision_timestamp = recruit.pending_shop_timestamp.take().or(timestamp);
            let cause = recruit.pending_shop_cause;
            recruit.pending_shop_cause = ShopRevisionCause::StateChange;
            recruit.shop_revisions.push(ShopRevision {
                revision: revision_number,
                timestamp: revision_timestamp,
                cause,
                cards: shop.clone(),
            });
            emitted = Some((recruit.round_number, revision_number, shop));
        }

        if let Some((round_number, revision, cards)) = emitted {
            events.push(HarnessEvent::ShopUpdated {
                round_number,
                revision,
                card_ids: cards.into_iter().filter_map(|card| card.card_id).collect(),
            });
        }
    }

    fn sync_timer_from_state(&mut self, event: &PowerEvent) {
        if self.current_raw_turn.map(|turn| turn % 2 == 1) != Some(true) {
            return;
        }
        let timeout_event = matches!(event, PowerEvent::TagChanged { tag, .. } if tag == "TIMEOUT");
        if timeout_event {
            self.sync_timer_anchor();
        }
    }

    fn sync_timer_anchor(&mut self) {
        let timeout = self.projector().timeout_seconds();
        if timeout.is_some() && timeout != self.timer_timeout_seconds {
            self.timer_timeout_seconds = timeout;
            self.timer_anchor = Some(Instant::now());
        } else if timeout.is_some() && self.timer_anchor.is_none() {
            self.timer_timeout_seconds = timeout;
            self.timer_anchor = Some(Instant::now());
        }
    }




    fn ensure_combat_started(&mut self, timestamp: Option<String>) {
        let Some(turn) = self.current_raw_turn else {
            return;
        };
        if turn % 2 == 1 {
            return;
        }
        let needs_start = self
            .current_combat
            .as_ref()
            .map(|c| c.start.is_none())
            .unwrap_or(false);
        if !needs_start {
            return;
        }

        let snapshot = self
            .projector()
            .combat_snapshot(round_number(turn), turn, timestamp);
        let player_id = snapshot.player.hero.as_ref().map(|h| h.entity_id);
        let opponent_id = snapshot.opponent.hero.as_ref().map(|h| h.entity_id);
        let player_effective = snapshot.player.hero.as_ref().and_then(|h| h.effective_health);
        let opponent_effective = snapshot.opponent.hero.as_ref().and_then(|h| h.effective_health);

        if let Some(combat) = self.current_combat.as_mut() {
            combat.player_hero_entity_id = player_id;
            combat.opponent_hero_entity_id = opponent_id;
            combat.player_effective_start = player_effective;
            combat.opponent_effective_start = opponent_effective;
            combat.player_effective_last = player_effective;
            combat.opponent_effective_last = opponent_effective;
            combat.start = Some(snapshot);
        }
    }

    fn observe_combat_tag(&mut self, event: &PowerEvent) {
        if self.current_raw_turn.map(|turn| turn % 2 == 0) != Some(true) {
            return;
        }
        let PowerEvent::TagChanged { target, tag, value } = event else {
            return;
        };
        let Some(combat) = self.current_combat.as_mut() else {
            return;
        };

        let local_player_id = self.store.local_player_id();
        let local_player_entity_id = local_player_id
            .and_then(|id| self.store.player(id))
            .map(|player| player.entity_id);
        let local_name = self.store.local_player_name();

        let local_target = match target {
            TagTarget::PlayerName(name) => local_name.map(|local| local == name).unwrap_or(false),
            TagTarget::Entity(identity) => {
                Some(identity.id) == local_player_entity_id
                    || Some(identity.id) == combat.player_hero_entity_id
            }
            TagTarget::GameEntity => false,
        };

        if tag == "BACON_WON_LAST_COMBAT" && local_target {
            if let Some(value) = value.as_i64() {
                combat.won_last_combat = Some(value != 0);
            }
        }

        if tag == "DAMAGE_DEALT_TO_HERO_LAST_TURN" && local_target {
            if let Some(value) = value.as_i64().and_then(|v| i32::try_from(v).ok()) {
                combat.explicit_damage_taken = Some(value.max(0));
            }
        }

        if tag == "PREDAMAGE" {
            let Some(value) = value.as_i64().and_then(|v| i32::try_from(v).ok()) else {
                return;
            };
            if value <= 0 {
                return;
            }
            if let TagTarget::Entity(identity) = target {
                if Some(identity.id) == combat.player_hero_entity_id {
                    combat.player_predamage_max = Some(
                        combat.player_predamage_max.unwrap_or(0).max(value),
                    );
                }
                if Some(identity.id) == combat.opponent_hero_entity_id {
                    combat.opponent_predamage_max = Some(
                        combat.opponent_predamage_max.unwrap_or(0).max(value),
                    );
                }
            }
        }
    }

    fn refresh_combat_health_tracking(&mut self) {
        let (player_id, opponent_id, has_start) = match self.current_combat.as_ref() {
            Some(combat) => (
                combat.player_hero_entity_id,
                combat.opponent_hero_entity_id,
                combat.start.is_some(),
            ),
            None => return,
        };
        if !has_start {
            return;
        }

        let player_health = player_id.and_then(|id| {
            self.store
                .entity(id)
                .map(|entity| HeroSnapshot::from_entity_with_catalog(entity, Some(&self.catalog)))
                .and_then(|hero| hero.effective_health)
        });
        let opponent_health = opponent_id.and_then(|id| {
            self.store
                .entity(id)
                .map(|entity| HeroSnapshot::from_entity_with_catalog(entity, Some(&self.catalog)))
                .and_then(|hero| hero.effective_health)
        });

        if let Some(combat) = self.current_combat.as_mut() {
            if player_health.is_some() {
                combat.player_effective_last = player_health;
            }
            if opponent_health.is_some() {
                combat.opponent_effective_last = opponent_health;
            }
        }
    }

    fn finalize_current_phase(&mut self, timestamp: Option<String>, events: &mut Vec<HarnessEvent>) {
        let Some(turn) = self.current_raw_turn else {
            return;
        };
        if turn % 2 == 1 {
            self.finalize_recruit(timestamp);
        } else {
            self.finalize_combat(timestamp, events);
        }
    }

    fn finalize_recruit(&mut self, timestamp: Option<String>) {
        let Some(turn) = self.current_raw_turn else { return; };
        let unresolved = self.drain_choices_for_turn(turn, timestamp.clone());
        let Some(mut recruit) = self.current_recruit.take() else { return; };
        recruit.choices.extend(unresolved);

        let mut end = self.recruit_snapshot(
            recruit.round_number,
            recruit.raw_game_turn,
            timestamp.clone(),
        );
        // At phase teardown Bob removes shop entities one-by-one. Always use
        // the last *published stable* shop rather than a transient 1/6-card
        // projector state observed during cleanup.
        if !recruit.last_stable_shop.is_empty() {
            end.shop = recruit.last_stable_shop.clone();
        }
        let mut start = recruit.start.take().unwrap_or_else(|| end.clone());
        if start.shop.is_empty() {
            if let Some(first) = recruit.shop_revisions.first() {
                start.shop = first.cards.clone();
            }
        }

        let archive = RecruitArchive {
            round_number: recruit.round_number,
            raw_game_turn: recruit.raw_game_turn,
            started_at: recruit.started_at,
            ended_at: timestamp,
            start,
            actions: recruit.actions,
            shop_revisions: recruit.shop_revisions,
            choices: recruit.choices,
            end,
        };
        self.pending_recruit_archive = Some(archive);
    }


    fn finalize_combat(&mut self, timestamp: Option<String>, events: &mut Vec<HarnessEvent>) {
        let Some(turn) = self.current_raw_turn else {
            return;
        };
        let unresolved = self.drain_choices_for_turn(turn, timestamp.clone());
        let Some(mut combat) = self.current_combat.take() else {
            return;
        };
        combat.choices.extend(unresolved);
        if combat.start.is_none() {
            // Bye/edge-case: no ATTACK block. Keep a best-effort snapshot.
            let snapshot = self.projector().combat_snapshot(
                combat.round_number,
                combat.raw_game_turn,
                combat.started_at.clone(),
            );
            combat.player_effective_start = snapshot.player.hero.as_ref().and_then(|h| h.effective_health);
            combat.opponent_effective_start = snapshot.opponent.hero.as_ref().and_then(|h| h.effective_health);
            combat.player_effective_last = combat.player_effective_start;
            combat.opponent_effective_last = combat.opponent_effective_start;
            combat.player_hero_entity_id = snapshot.player.hero.as_ref().map(|h| h.entity_id);
            combat.opponent_hero_entity_id = snapshot.opponent.hero.as_ref().map(|h| h.entity_id);
            combat.start = Some(snapshot);
        }

        let start = combat.start.expect("combat start was populated");
        let health_damage_taken = positive_drop(combat.player_effective_start, combat.player_effective_last);
        let health_damage_dealt = positive_drop(combat.opponent_effective_start, combat.opponent_effective_last);

        let damage_taken = combat
            .explicit_damage_taken
            .or(combat.player_predamage_max)
            .or(health_damage_taken);
        let damage_dealt = combat.opponent_predamage_max.or(health_damage_dealt);

        let result = match combat.won_last_combat {
            Some(true) => CombatResult::Win,
            Some(false) if damage_taken.unwrap_or(0) > 0 => CombatResult::Loss,
            Some(false) => CombatResult::Tie,
            None => match (damage_taken.unwrap_or(0) > 0, damage_dealt.unwrap_or(0) > 0) {
                (true, false) => CombatResult::Loss,
                (false, true) => CombatResult::Win,
                (false, false) => CombatResult::Tie,
                (true, true) => CombatResult::Unknown,
            },
        };

        let archive = CombatArchive {
            round_number: combat.round_number,
            raw_game_turn: combat.raw_game_turn,
            started_at: start.timestamp.clone().or(combat.started_at),
            ended_at: timestamp,
            opponent_player_id: start.opponent_player_id,
            lobby: start.lobby,
            player: start.player,
            opponent: start.opponent,
            result,
            damage_taken,
            damage_dealt,
            choices: combat.choices,
        };

        let recruit = self
            .pending_recruit_archive
            .take()
            .filter(|r| r.round_number == combat.round_number);
        let round = RoundArchive {
            round_number: combat.round_number,
            recruit,
            combat: Some(archive),
        };
        self.upsert_round(round);
        events.push(HarnessEvent::RoundArchived {
            round_number: combat.round_number,
        });
    }

    fn push_pending_recruit_as_partial_round(&mut self, events: &mut Vec<HarnessEvent>) {
        let Some(recruit) = self.pending_recruit_archive.take() else {
            return;
        };
        let round_number = recruit.round_number;
        self.upsert_round(RoundArchive {
            round_number,
            recruit: Some(recruit),
            combat: None,
        });
        events.push(HarnessEvent::RoundArchived { round_number });
    }

    fn upsert_round(&mut self, round: RoundArchive) {
        if let Some(existing) = self
            .archive
            .rounds
            .iter_mut()
            .find(|r| r.round_number == round.round_number)
        {
            *existing = round;
        } else {
            self.archive.rounds.push(round);
            self.archive.rounds.sort_by_key(|r| r.round_number);
        }
    }

    fn handle_user_option_sent(
        &mut self,
        selected_option: i32,
        selected_target: Option<u32>,
        selected_position: Option<i32>,
        timestamp: Option<String>,
        events: &mut Vec<HarnessEvent>,
    ) {
        if !self.active || self.current_raw_turn.map(|turn| turn % 2 == 1) != Some(true) {
            self.pending_user_input = None;
            return;
        }

        self.finalize_pending_recruit_action(timestamp.clone());
        // A new user input is also an authoritative stable boundary for the
        // state created by the previous action. This complements the live
        // debounce path and makes offline replay deterministic.
        self.flush_shop_revision(timestamp.clone(), events);
        self.try_start_recruit(timestamp.clone(), true);
        self.pending_user_input = Some(PendingUserInput {
            timestamp,
            selected_option,
            selected_target,
            selected_position,
        });
    }

    fn finalize_pending_recruit_action(&mut self, timestamp: Option<String>) {
        let Some(action) = self.pending_recruit_action.take() else {
            return;
        };
        let Some(turn) = self.current_raw_turn else {
            return;
        };
        if turn % 2 == 0 {
            return;
        }

        self.try_start_recruit(action.timestamp.clone(), true);
        let snapshot = self.recruit_snapshot(round_number(turn), turn, timestamp);
        if let Some(recruit) = self.current_recruit.as_mut() {
            recruit.actions.push(RecruitActionRecord {
                timestamp: action.timestamp,
                kind: action.kind,
                source: action.source,
                target: action.target,
                state_after: snapshot,
            });
        }
    }

    fn detect_recruit_action(
        &self,
        event: &PowerEvent,
        timestamp: Option<String>,
        selected_target: Option<u32>,
    ) -> Option<PendingAction> {
        let turn = self.current_raw_turn?;
        if turn % 2 == 0 { return None; }
        let PowerEvent::BlockStarted { block_type, source, target, .. } = event else { return None; };

        let source_snapshot = source.as_ref().map(|identity| self.snapshot_from_identity(identity));
        let target_snapshot = target
            .as_ref()
            .map(|identity| self.snapshot_from_identity(identity))
            .or_else(|| {
                selected_target
                    .and_then(|entity_id| self.store.entity(entity_id))
                    .map(|entity| CardSnapshot::from_entity_with_catalog(entity, Some(&self.catalog)))
            });
        let source_entity = source.as_ref().and_then(|identity| self.store.entity(identity.id));
        let source_card_id = source
            .as_ref()
            .and_then(|identity| identity.card_id.as_deref())
            .or_else(|| source_entity.and_then(|entity| entity.card_id.as_deref()));
        let source_type = source_entity
            .and_then(|entity| entity.tag_symbol("CARDTYPE"))
            .or_else(|| source_snapshot.as_ref().and_then(|card| card.card_type.as_deref()));
        let source_zone = source_entity
            .and_then(|entity| entity.tag_symbol("ZONE"))
            .or_else(|| source_snapshot.as_ref().and_then(|card| card.zone.as_deref()));
        let local = self.store.local_player_id();
        let source_controller = source_entity
            .and_then(|entity| entity.tag_i64("CONTROLLER"))
            .and_then(|value| i32::try_from(value).ok())
            .or_else(|| source_snapshot.as_ref().and_then(|card| card.controller));

        let kind = match block_type.as_str() {
            "MOVE_MINION" if source_controller == local => RecruitActionKind::Reorder,
            "ATTACK" if source_controller == local => RecruitActionKind::RecruitAttack,
            "POWER" if source_controller == local && source_type == Some("HERO_POWER") => {
                RecruitActionKind::UseHeroPower
            }
            "POWER" if source_controller == local
                && source_snapshot
                    .as_ref()
                    .map(|card| {
                        card.keywords.activate_keyword && card.keywords.activate_available_now
                    })
                    .unwrap_or(false) => RecruitActionKind::Activate,
            "POWER" if source_controller == local && source_type == Some("GAME_MODE_BUTTON") => {
                RecruitActionKind::SpecialAction
            }
            "PLAY" => match source_card_id {
                Some("TB_BaconShop_8p_Reroll_Button") | Some("TB_BaconShop_8P_Reroll_Button") => RecruitActionKind::Refresh,
                Some("TB_BaconShop_DragBuy") => RecruitActionKind::BuyMinion,
                Some("TB_BaconShop_DragBuy_Spell") => RecruitActionKind::BuySpell,
                Some("TB_BaconShop_DragSell") => RecruitActionKind::SellMinion,
                Some("TB_BaconShopLockAll_Button") => RecruitActionKind::FreezeToggle,
                Some(id) if id.starts_with("TB_BaconShopTechUp") => RecruitActionKind::TavernUpgrade,
                _ if source_controller == local
                    && source_zone == Some("PLAY")
                    && source_type == Some("MINION")
                    && source_snapshot
                        .as_ref()
                        .map(|card| card.keywords.activate_keyword && card.keywords.activate_available_now)
                        .unwrap_or(false) => RecruitActionKind::Activate,
                _ if source_controller == local && source_zone == Some("HAND") && source_type == Some("MINION") => RecruitActionKind::PlayMinion,
                _ if source_controller == local && source_zone == Some("HAND") && matches!(source_type, Some("BATTLEGROUND_SPELL") | Some("SPELL")) => RecruitActionKind::CastSpell,
                _ if source_controller == local && source_type == Some("HERO_POWER") => RecruitActionKind::UseHeroPower,
                _ if source_controller == local && source_type == Some("GAME_MODE_BUTTON") => RecruitActionKind::SpecialAction,
                _ if source_controller == local && source_zone == Some("HAND") => RecruitActionKind::PlayCard,
                _ => return None,
            },
            _ => return None,
        };


        Some(PendingAction {
            timestamp,
            kind,
            source: source_snapshot,
            target: target_snapshot,
        })
    }


    fn recruit_snapshot(
        &self,
        round_number: u32,
        raw_game_turn: u32,
        timestamp: Option<String>,
    ) -> RecruitSnapshot {
        let mut snapshot = self
            .projector()
            .recruit_snapshot(round_number, raw_game_turn, timestamp);
        snapshot.shop_frozen = self.shop_frozen_state.or(snapshot.shop_frozen);
        snapshot
    }

    fn bootstrap_shop_frozen_state(&mut self) {
        if self.shop_frozen_state.is_none() {
            let observed = self.projector().shop_frozen().or(Some(false));
            self.shop_frozen_state = observed;
        }
    }

    fn apply_recruit_action_state(&mut self, kind: Option<&RecruitActionKind>) {
        match kind {
            Some(RecruitActionKind::FreezeToggle) => {
                let current = self
                    .shop_frozen_state
                    .or_else(|| self.projector().shop_frozen())
                    .unwrap_or(false);
                self.shop_frozen_state = Some(!current);
            }
            // Rerolling replaces the frozen shop and leaves the newly rolled
            // shop unfrozen. This is also a useful recovery if a freeze tag was
            // missing earlier in the turn.
            Some(RecruitActionKind::Refresh) => {
                self.shop_frozen_state = Some(false);
            }
            _ => {}
        }
    }

    fn recover_block_stack(&mut self) {
        if !self.block_stack.is_empty() {
            self.block_stack.clear();
            self.stale_block_stack_recoveries =
                self.stale_block_stack_recoveries.saturating_add(1);
        }
    }

    fn refresh_metadata(&mut self) {
        let lobby_players = self.projector().lobby_players();
        let local_player_id = self.store.local_player_id();
        let final_place = local_player_id
            .and_then(|player_id| self.store.leaderboard_place(player_id))
            .and_then(|place| u8::try_from(place).ok());

        self.archive.local_player_id = local_player_id;
        self.archive.local_player_name = self.store.local_player_name().map(str::to_owned);
        self.archive.lobby_players = lobby_players;
        self.archive.final_place = final_place;
        self.archive.stale_block_stack_recoveries = self.stale_block_stack_recoveries;
    }
}

fn shop_cause_for_action(kind: &RecruitActionKind) -> Option<ShopRevisionCause> {
    match kind {
        RecruitActionKind::Refresh => Some(ShopRevisionCause::Refresh),
        RecruitActionKind::BuyMinion | RecruitActionKind::BuySpell => Some(ShopRevisionCause::Buy),
        RecruitActionKind::SellMinion => Some(ShopRevisionCause::Sell),
        RecruitActionKind::CastSpell
        | RecruitActionKind::PlayMinion
        | RecruitActionKind::PlayCard
        | RecruitActionKind::Activate
        | RecruitActionKind::SpecialAction
        | RecruitActionKind::UseHeroPower => Some(ShopRevisionCause::SpellOrEffect),
        RecruitActionKind::TavernUpgrade => Some(ShopRevisionCause::OtherAction),
        RecruitActionKind::FreezeToggle
        | RecruitActionKind::Reorder
        | RecruitActionKind::RecruitAttack => None,
    }
}

fn game_turn_from_event(event: &PowerEvent) -> Option<u32> {
    let PowerEvent::TagChanged {
        target: TagTarget::GameEntity,
        tag,
        value: TagValue::Int(value),
    } = event
    else {
        return None;
    };
    if tag != "TURN" {
        return None;
    }
    u32::try_from(*value).ok()
}

fn round_number(raw_turn: u32) -> u32 {
    (raw_turn + 1) / 2
}

fn positive_drop(start: Option<i32>, end: Option<i32>) -> Option<i32> {
    match (start, end) {
        (Some(start), Some(end)) => Some((start - end).max(0)),
        _ => None,
    }
}

fn shop_signature(cards: &[CardSnapshot]) -> Vec<(u32, i32, String)> {
    cards
        .iter()
        .map(|card| {
            (
                card.entity_id,
                card.zone_position.unwrap_or(i32::MAX),
                card.card_id.clone().unwrap_or_default(),
            )
        })
        .collect()
}

fn trustworthy_name(name: Option<&str>) -> Option<&str> {
    let name = name?.trim();
    if name.is_empty()
        || name.eq_ignore_ascii_case("unknown")
        || name.to_ascii_uppercase().starts_with("UNKNOWN ENTITY")
    {
        None
    } else {
        Some(name)
    }
}

fn classify_choice(
    choice_type: &str,
    source_card_id: Option<&str>,
    source_name: Option<&str>,
    options: &[CardSnapshot],
) -> ChoiceKind {
    if choice_type == "MULLIGAN" {
        return ChoiceKind::Hero;
    }

    let source_id = source_card_id.unwrap_or("").to_ascii_uppercase();
    let source_name_lower = source_name.unwrap_or("").to_ascii_lowercase();
    let option_is_trinket = options.iter().any(|option| {
        option
            .card_type
            .as_deref()
            .map(|value| value.to_ascii_uppercase().contains("TRINKET"))
            .unwrap_or(false)
            || option
                .card_id
                .as_deref()
                .map(|value| value.to_ascii_uppercase().contains("TRINKET"))
                .unwrap_or(false)
    });
    if source_id.contains("TRINKET")
        || source_name_lower.contains("trinket")
        || source_name_lower.contains("饰品")
        || option_is_trinket
    {
        return ChoiceKind::Trinket;
    }

    let option_is_dark_gift = options.iter().any(|option| {
        option
            .card_type
            .as_deref()
            .map(|value| {
                let upper = value.to_ascii_uppercase();
                upper.contains("DARK_GIFT") || upper.contains("DARKGIFT")
            })
            .unwrap_or(false)
            || option
                .card_id
                .as_deref()
                .map(|value| {
                    let upper = value.to_ascii_uppercase();
                    upper.contains("DARK_GIFT") || upper.contains("DARKGIFT")
                })
                .unwrap_or(false)
    });
    if source_id.contains("DARK_GIFT")
        || source_id.contains("DARKGIFT")
        || source_name_lower.contains("dark gift")
        || source_name_lower.contains("黑暗赠礼")
        || option_is_dark_gift
    {
        return ChoiceKind::DarkGift;
    }

    if choice_type == "GENERAL" {
        ChoiceKind::Discover
    } else {
        ChoiceKind::Other
    }
}
