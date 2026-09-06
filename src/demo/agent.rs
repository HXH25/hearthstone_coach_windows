use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::harness::{CardCatalog, CardSnapshot, HarnessRuntime, PhaseKind, RecruitSnapshot};

use super::model::{
    ActionRecommendation, DecisionWeights, ReplanEvent, ReplanLevel, RoundPlan, TacticalPlan,
    TrinketPlan, UpgradePosture, WatchlistResponse,
};

/// Compact, stable context passed to the strategic planner. It deliberately
/// contains only projected Harness facts; the Agent never reads EntityStore or
/// Power.log directly.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentSnapshot {
    pub round_number: u32,
    pub phase: String,
    pub hero_effective_health: Option<i32>,
    pub gold: Option<i32>,
    pub tavern_tier: Option<u8>,
    pub refresh_cost: Option<i32>,
    pub upgrade_cost: Option<i32>,
    pub next_opponent_player_id: Option<i32>,
    pub available_tribes: Vec<String>,
    pub board: Vec<AgentCard>,
    pub hand: Vec<AgentCard>,
    pub shop: Vec<AgentCard>,
    pub trinkets: Vec<AgentCard>,
    pub opponent_board: Vec<AgentCard>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentCard {
    pub entity_id: u32,
    pub card_id: String,
    pub name: String,
    pub attack: Option<i32>,
    pub health: Option<i32>,
    pub tavern_tier: Option<u8>,
    pub is_golden: bool,
    pub tribes: Vec<String>,
    pub text: String,
    pub mechanics: Vec<String>,
}

impl From<&CardSnapshot> for AgentCard {
    fn from(card: &CardSnapshot) -> Self {
        Self {
            entity_id: card.entity_id,
            card_id: card.card_id.clone().unwrap_or_default(),
            name: card.name.clone().or_else(|| card.card_id.clone()).unwrap_or_else(|| "unknown".to_owned()),
            attack: card.attack,
            health: card.remaining_health.or(card.health),
            tavern_tier: card.tavern_tier,
            is_golden: card.is_golden,
            tribes: card.tribes.clone(),
            text: card.text.clone().unwrap_or_default(),
            mechanics: card.mechanics.clone(),
        }
    }
}

impl AgentSnapshot {
    pub fn capture(runtime: &HarnessRuntime, available_tribes: &[String]) -> Self {
        match runtime.current_phase_kind() {
            PhaseKind::Recruit => runtime
                .current_recruit_state()
                .map(|snapshot| Self::from_recruit(snapshot, available_tribes))
                .unwrap_or_else(|| Self::fallback(runtime, available_tribes, "Recruit")),
            PhaseKind::Combat => {
                let combat = runtime.current_combat_state();
                let round = runtime.current_round_number().unwrap_or(0);
                let recruit_end = runtime
                    .round_archive(round)
                    .and_then(|round| round.recruit.as_ref())
                    .map(|recruit| recruit.end.clone());
                let mut out = recruit_end
                    .map(|snapshot| Self::from_recruit(snapshot, available_tribes))
                    .unwrap_or_else(|| Self::fallback(runtime, available_tribes, "Combat"));
                out.phase = "Combat".to_owned();
                out.round_number = round;
                if let Some(combat) = combat {
                    out.hero_effective_health = combat.player.hero.as_ref().and_then(|hero| hero.effective_health);
                    out.board = combat.player.board.iter().map(AgentCard::from).collect();
                    out.opponent_board = combat.opponent.board.iter().map(AgentCard::from).collect();
                    out.next_opponent_player_id = combat.opponent_player_id;
                }
                out
            }
            phase => Self::fallback(runtime, available_tribes, &format!("{phase:?}")),
        }
    }

    fn from_recruit(snapshot: RecruitSnapshot, available_tribes: &[String]) -> Self {
        Self {
            round_number: snapshot.round_number,
            phase: "Recruit".to_owned(),
            hero_effective_health: snapshot.hero.as_ref().and_then(|hero| hero.effective_health),
            gold: snapshot.gold,
            tavern_tier: snapshot.tavern_tier,
            refresh_cost: snapshot.refresh_cost,
            upgrade_cost: snapshot.upgrade_cost,
            next_opponent_player_id: snapshot.next_opponent_player_id,
            available_tribes: available_tribes.to_vec(),
            board: snapshot.board.iter().map(AgentCard::from).collect(),
            hand: snapshot.hand.iter().map(AgentCard::from).collect(),
            shop: snapshot.shop.iter().map(AgentCard::from).collect(),
            trinkets: snapshot.trinkets.iter().map(AgentCard::from).collect(),
            opponent_board: Vec::new(),
        }
    }

    fn fallback(runtime: &HarnessRuntime, available_tribes: &[String], phase: &str) -> Self {
        Self {
            round_number: runtime.current_round_number().unwrap_or(0),
            phase: phase.to_owned(),
            hero_effective_health: runtime.current_hero().and_then(|hero| hero.effective_health),
            gold: runtime.current_gold(),
            tavern_tier: runtime.current_tavern_tier(),
            refresh_cost: runtime.current_refresh_cost(),
            upgrade_cost: runtime.current_upgrade_cost(),
            next_opponent_player_id: runtime.next_opponent_player_id(),
            available_tribes: available_tribes.to_vec(),
            board: runtime.current_board().iter().map(AgentCard::from).collect(),
            hand: runtime.current_hand().iter().map(AgentCard::from).collect(),
            shop: runtime.stable_shop().iter().map(AgentCard::from).collect(),
            trinkets: runtime.current_trinkets().iter().map(AgentCard::from).collect(),
            opponent_board: Vec::new(),
        }
    }

    pub fn dominant_tribe(&self) -> Option<String> {
        let mut counts = BTreeMap::<String, usize>::new();
        for card in &self.board {
            for tribe in &card.tribes {
                *counts.entry(tribe.clone()).or_default() += 1;
            }
        }
        counts.into_iter().max_by_key(|(_, count)| *count).map(|(tribe, _)| tribe)
    }

    pub fn owned_card_ids(&self) -> BTreeSet<String> {
        self.board
            .iter()
            .chain(self.hand.iter())
            .filter(|card| !card.card_id.is_empty())
            .map(|card| card.card_id.clone())
            .collect()
    }
}

/// Stable facts used to detect whether the slow strategic plan has been
/// invalidated. This is intentionally tiny and deterministic.
#[derive(Debug, Clone, Default)]
pub struct DecisionFingerprint {
    pub effective_health: Option<i32>,
    pub tavern_tier: Option<u8>,
    pub trinket_count: usize,
    pub golden_count: usize,
    pub dominant_tribe: Option<String>,
    pub owned_card_ids: BTreeSet<String>,
}

impl DecisionFingerprint {
    pub fn from_snapshot(snapshot: &AgentSnapshot) -> Self {
        Self {
            effective_health: snapshot.hero_effective_health,
            tavern_tier: snapshot.tavern_tier,
            trinket_count: snapshot.trinkets.len(),
            golden_count: snapshot.board.iter().chain(snapshot.hand.iter()).filter(|card| card.is_golden).count(),
            dominant_tribe: snapshot.dominant_tribe(),
            owned_card_ids: snapshot.owned_card_ids(),
        }
    }
}

pub fn detect_replan(
    before: &DecisionFingerprint,
    now: &DecisionFingerprint,
    selected_core_cards: &[String],
    critical_health: i32,
    emergency_health: i32,
) -> Vec<ReplanEvent> {
    let mut events = Vec::new();

    if before.tavern_tier != now.tavern_tier && before.tavern_tier.is_some() {
        events.push(ReplanEvent::new(
            ReplanLevel::Strategic,
            "tavern_tier_changed",
            format!("酒馆等级 {:?} → {:?}", before.tavern_tier, now.tavern_tier),
        ));
    }

    if now.trinket_count > before.trinket_count {
        events.push(ReplanEvent::new(
            ReplanLevel::Strategic,
            "trinket_acquired",
            format!("饰品数量 {} → {}", before.trinket_count, now.trinket_count),
        ));
    }

    if now.golden_count > before.golden_count {
        events.push(ReplanEvent::new(
            ReplanLevel::Tactical,
            "triple_completed",
            format!("金色随从数量 {} → {}，重算本回合行动路线", before.golden_count, now.golden_count),
        ));
    }

    if before.dominant_tribe != now.dominant_tribe
        && before.dominant_tribe.is_some()
        && now.dominant_tribe.is_some()
    {
        events.push(ReplanEvent::new(
            ReplanLevel::Strategic,
            "board_direction_shift",
            format!("阵容主种族 {:?} → {:?}", before.dominant_tribe, now.dominant_tribe),
        ));
    }

    if let (Some(old), Some(new)) = (before.effective_health, now.effective_health) {
        if old > critical_health && new <= critical_health {
            events.push(ReplanEvent::new(
                ReplanLevel::Strategic,
                "critical_health",
                format!("有效生命降至 {new}，进入保命阈值"),
            ));
        }
        if old > emergency_health && new <= emergency_health {
            events.push(ReplanEvent::new(
                ReplanLevel::LongTerm,
                "emergency_health",
                format!("有效生命降至 {new}，长期贪成长策略应停止"),
            ));
        }
    }

    for card_id in selected_core_cards {
        if !before.owned_card_ids.contains(card_id) && now.owned_card_ids.contains(card_id) {
            events.push(ReplanEvent::new(
                ReplanLevel::Strategic,
                "core_card_acquired",
                format!("获得阵容核心 {card_id}"),
            ));
        }
    }

    events
}

/// Conservative tavern curve used only as a local valuation prior. It does not
/// try to hard-code one meta line; it simply makes old low-tier cards lose
/// relative value as the lobby reaches later turns.
pub fn expected_tavern_tier(round: u32) -> u8 {
    match round {
        0 | 1 => 1,
        2 | 3 => 2,
        4 | 5 => 3,
        6 | 7 => 4,
        8 | 9 => 5,
        _ => 6,
    }
}

/// Multiplier for a shop card's *current* relevance. By round 5 the reference
/// tier is T3, so a T1 card receives 0.50 while a T3 card stays at 1.00. This is
/// intentionally applied to priority/watchlist value as well as part of the
/// raw body value, which prevents an old early-game S card from permanently
/// outranking a current-tier A card.
pub fn tier_relevance(round: u32, current_tavern_tier: Option<u8>, card_tier: Option<u8>) -> f32 {
    let card_tier = card_tier.unwrap_or(1).clamp(1, 6);
    let reference = expected_tavern_tier(round)
        .max(current_tavern_tier.unwrap_or(1))
        .clamp(1, 6);
    if card_tier >= reference {
        (1.0 + 0.08 * (card_tier - reference) as f32).min(1.18)
    } else {
        (1.0 - 0.25 * (reference - card_tier) as f32).clamp(0.35, 1.0)
    }
}

/// An item pulled only from an older Watchlist stage should not keep its old
/// S/A bonus forever. Cards explicitly repeated in the current stage receive
/// full value.
fn watch_stage_relevance(round: u32, watch_stage: &str) -> f32 {
    let current = match round {
        0..=4 => "early",
        5..=8 => "mid",
        _ => "late",
    };
    match (current, watch_stage) {
        (a, b) if a == b => 1.0,
        ("mid", "early") => 0.68,
        ("late", "mid") => 0.72,
        ("late", "early") => 0.45,
        _ => 0.85,
    }
}

/// Layer 3 + Layer 2 local execution: rank immediate actions and then choose a
/// short known-information route. Refresh is terminal because its future shop
/// is unknown; we score it by an explicit expected-value prior rather than
/// pretending to know the next roll.
pub fn evaluate_recruit(
    snapshot: &AgentSnapshot,
    round_plan: Option<&RoundPlan>,
    trinket_plan: Option<&TrinketPlan>,
    watchlist: Option<&WatchlistResponse>,
    catalog: &CardCatalog,
    refreshes_used: u8,
    shop_revision: u32,
) -> TacticalPlan {
    let weights = round_plan.map(|plan| plan.weights.normalized()).unwrap_or_default();
    let gold = snapshot.gold.unwrap_or(0).max(0);
    let reserve = round_plan.map(|plan| plan.economy.gold_reserve.max(0)).unwrap_or(0).min(gold);
    let spendable_gold = (gold - reserve).max(0);
    let mut actions = Vec::new();

    for (index, card) in snapshot.shop.iter().enumerate() {
        let cost = purchase_cost(card, catalog);
        if cost > spendable_gold {
            continue;
        }
        let score = card_score(card, snapshot, round_plan, watchlist, &weights);
        actions.push(ActionRecommendation {
            kind: "buy".to_owned(),
            score,
            label: format!("购买 {}", card.name),
            reason: explain_card_score(card, score, snapshot.round_number, round_plan, watchlist),
            card_id: nonempty(&card.card_id),
            shop_index: Some(index),
            gold_after: Some(gold - cost),
        });
    }

    if snapshot.board.len() < 7 {
        for card in &snapshot.hand {
            let score = board_keep_score(card, snapshot, round_plan, &weights) + 0.45 * weights.tempo;
            actions.push(ActionRecommendation {
                kind: "play".to_owned(),
                score,
                label: format!("打出 {}", card.name),
                reason: format!("把手牌转成场面价值（本地评分 {:.1}）", score),
                card_id: nonempty(&card.card_id),
                shop_index: None,
                gold_after: Some(gold),
            });
        }
    }

    if snapshot.board.len() >= 7 {
        if let Some((sell_index, weakest)) = snapshot
            .board
            .iter()
            .enumerate()
            .map(|(index, card)| (index, card, board_keep_score(card, snapshot, round_plan, &weights)))
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(index, card, _)| (index, card))
        {
            if let Some(best_buy) = actions.iter().filter(|action| action.kind == "buy").max_by(|a, b| a.score.total_cmp(&b.score)) {
                if best_buy.score > 6.0 {
                    actions.push(ActionRecommendation {
                        kind: "sell_then_buy".to_owned(),
                        score: best_buy.score - 0.6,
                        label: format!("卖 {} → {}", weakest.name, best_buy.label),
                        reason: format!("场上已满；{} 是当前最低保留价值位置", weakest.name),
                        card_id: best_buy.card_id.clone(),
                        shop_index: best_buy.shop_index,
                        gold_after: best_buy.gold_after.map(|value| value + 1),
                    });
                }
            }
            let _ = sell_index;
        }
    }

    if let Some(cost) = snapshot
        .upgrade_cost
        .filter(|cost| *cost >= 0 && *cost <= gold)
    {
        let current_tier = snapshot.tavern_tier.unwrap_or(1).clamp(1, 6);
        let curve_tier = expected_tavern_tier(snapshot.round_number);
        let behind_curve = curve_tier.saturating_sub(current_tier) as f32;
        let posture = round_plan
            .map(|plan| plan.tier_policy.posture)
            .unwrap_or(UpgradePosture::Neutral);
        let posture_bonus = match posture {
            UpgradePosture::Prefer => 2.0,
            UpgradePosture::Neutral => 0.35,
            UpgradePosture::Delay => -1.35,
        };
        let hp = snapshot.hero_effective_health.unwrap_or(30);
        let risk_penalty = if hp <= 8 {
            4.4
        } else if hp <= 15 {
            2.1
        } else {
            0.0
        };
        let best_buy = actions
            .iter()
            .filter(|action| action.kind == "buy")
            .map(|action| action.score)
            .fold(0.0_f32, f32::max);
        let shop_opportunity_penalty = (best_buy - 7.0).max(0.0) * 0.42;
        // gold_reserve is also a soft strategic preference. Falling behind the
        // tier curve must still be able to recommend an upgrade that spends
        // into the reserve instead of silently removing Upgrade from legal
        // actions altogether.
        let reserve_breach = (cost - spendable_gold).max(0) as f32;
        let reserve_penalty_per_gold = match posture {
            UpgradePosture::Prefer => 0.35,
            UpgradePosture::Neutral => 0.70,
            UpgradePosture::Delay => 1.10,
        };
        let cheap_upgrade_bonus = (5 - cost).max(0) as f32 * 0.28;
        let score = 4.15
            + behind_curve * 2.15
            + posture_bonus
            + cheap_upgrade_bonus
            + weights.scaling * 0.75
            + weights.economy * 0.28
            - weights.tempo * 0.18
            - risk_penalty
            - shop_opportunity_penalty
            - reserve_breach * reserve_penalty_per_gold;
        actions.push(ActionRecommendation {
            kind: "upgrade".to_owned(),
            score,
            label: "升级酒馆".to_owned(),
            reason: format!(
                "当前T{current_tier} / 回合曲线T{curve_tier}；策略={posture:?}；落后曲线奖励 {:.1}；动用保留金币 {:.0}",
                behind_curve * 2.15,
                reserve_breach
            ),
            card_id: None,
            shop_index: None,
            gold_after: Some(gold - cost),
        });
    }

    let max_rerolls = round_plan.map(|plan| plan.economy.max_rerolls).unwrap_or(2);
    if refreshes_used < max_rerolls {
        if let Some(cost) = snapshot.refresh_cost.filter(|cost| *cost >= 0 && *cost <= spendable_gold) {
            let best_buy = actions
                .iter()
                .filter(|action| action.kind == "buy")
                .map(|action| action.score)
                .fold(0.0_f32, f32::max);
            let score = 4.2 + weights.scaling * 0.35 + weights.synergy * 0.45 - (best_buy - 5.0).max(0.0) * 0.55;
            actions.push(ActionRecommendation {
                kind: "refresh".to_owned(),
                score,
                label: "刷新商店".to_owned(),
                reason: format!("刷新预算 {refreshes_used}/{max_rerolls}；新商店未知，按期望价值评分"),
                card_id: None,
                shop_index: None,
                gold_after: Some(gold - cost),
            });
        }
    }

    if should_freeze(snapshot, round_plan, watchlist, &weights, catalog) {
        actions.push(ActionRecommendation {
            kind: "freeze".to_owned(),
            score: 5.4 + weights.synergy * 0.5,
            label: "冻结商店".to_owned(),
            reason: "存在当前金币买不起但符合小目标的高价值牌".to_owned(),
            card_id: None,
            shop_index: None,
            gold_after: Some(gold),
        });
    }

    let _ = trinket_plan; // role priority is consumed by the Choice UI/planner, not as a fake Recruit action.

    actions.push(ActionRecommendation {
        kind: "hold".to_owned(),
        score: 1.0,
        label: "保持当前状态".to_owned(),
        reason: if reserve > 0 { format!("当前计划要求至少保留 {reserve} 金币") } else { "没有更高价值的确定动作时停止过度操作".to_owned() },
        card_id: None,
        shop_index: None,
        gold_after: Some(gold),
    });

    actions.sort_by(|a, b| b.score.total_cmp(&a.score));
    actions.truncate(6);

    let route = best_known_route(snapshot, &actions, catalog);
    let summary = route
        .first()
        .map(|action| action.label.clone())
        .unwrap_or_else(|| "等待稳定状态".to_owned());

    TacticalPlan {
        round_number: snapshot.round_number,
        shop_revision,
        summary,
        route,
        ranked_actions: actions,
    }
}

fn best_known_route(snapshot: &AgentSnapshot, actions: &[ActionRecommendation], catalog: &CardCatalog) -> Vec<ActionRecommendation> {
    let gold = snapshot.gold.unwrap_or(0).max(0);
    let buys: Vec<_> = actions.iter().filter(|action| action.kind == "buy").cloned().collect();
    let mut best_score = f32::NEG_INFINITY;
    let mut best = Vec::new();

    for first in &buys {
        let first_cost = first
            .shop_index
            .and_then(|index| snapshot.shop.get(index))
            .map(|card| purchase_cost(card, catalog))
            .unwrap_or(3);
        if first_cost > gold {
            continue;
        }
        if first.score > best_score {
            best_score = first.score;
            best = vec![first.clone()];
        }
        for second in &buys {
            if first.shop_index == second.shop_index {
                continue;
            }
            let second_cost = second
                .shop_index
                .and_then(|index| snapshot.shop.get(index))
                .map(|card| purchase_cost(card, catalog))
                .unwrap_or(3);
            if first_cost + second_cost <= gold {
                let score = first.score + second.score * 0.88;
                if score > best_score {
                    best_score = score;
                    best = vec![first.clone(), second.clone()];
                }
            }
        }
    }

    if let Some(single) = actions.first() {
        if single.score > best_score {
            best = vec![single.clone()];
        }
    }
    best
}

fn purchase_cost(card: &AgentCard, catalog: &CardCatalog) -> i32 {
    let card_type = catalog
        .resolve(&card.card_id)
        .and_then(|meta| meta.card_type())
        .unwrap_or("MINION");
    if card_type == "BATTLEGROUND_SPELL" {
        catalog.resolve(&card.card_id).and_then(|meta| meta.cost()).unwrap_or(1).max(0)
    } else {
        3
    }
}

fn card_score(
    card: &AgentCard,
    snapshot: &AgentSnapshot,
    round_plan: Option<&RoundPlan>,
    watchlist: Option<&WatchlistResponse>,
    weights: &DecisionWeights,
) -> f32 {
    let attack = card.attack.unwrap_or(0).max(0) as f32;
    let health = card.health.unwrap_or(0).max(0) as f32;
    let tier = card.tavern_tier.unwrap_or(1) as f32;
    let relevance = tier_relevance(snapshot.round_number, snapshot.tavern_tier, card.tavern_tier);
    let raw_stats = (attack + health).sqrt().min(12.0);
    let body_relevance = 0.62 + 0.38 * relevance;
    let mut score = 0.9
        + raw_stats * 0.35 * weights.tempo * body_relevance
        + tier * 0.42 * relevance;

    if card.is_golden {
        score += 2.0 + weights.triple;
    }
    if card.mechanics.iter().any(|m| m.eq_ignore_ascii_case("Divine Shield")) {
        score += 0.7 * weights.survival;
    }
    if card.mechanics.iter().any(|m| m.eq_ignore_ascii_case("Battlecry") || m.eq_ignore_ascii_case("Deathrattle")) {
        score += 0.35 * weights.synergy;
    }

    let desired: BTreeSet<_> = round_plan
        .map(|plan| plan.direction.iter().map(|item| item.to_ascii_uppercase()).collect())
        .unwrap_or_default();
    if card.tribes.iter().any(|tribe| desired.contains(&tribe.to_ascii_uppercase())) {
        score += 1.5 * weights.synergy;
    }
    if snapshot
        .dominant_tribe()
        .as_ref()
        .map(|tribe| card.tribes.iter().any(|item| item == tribe))
        .unwrap_or(false)
    {
        score += 0.8 * weights.synergy;
    }

    if let Some((watch_stage, watch)) = watch_for_card(watchlist, snapshot.round_number, &card.card_id) {
        let stage_relevance = watch_stage_relevance(snapshot.round_number, watch_stage);
        let priority_bonus = match watch.priority.as_str() {
            "S" => 4.5,
            "A" => 2.7,
            _ => 1.2,
        };
        score += priority_bonus * relevance * stage_relevance;
        let role = watch.role.to_ascii_lowercase();
        // Role bonuses decay more gently than raw priority: a low-tier genuine
        // scaling/core piece can still be correct, it just no longer wins only
        // because it was labeled S in the early-game list.
        let role_relevance = 0.72 + 0.28 * relevance;
        if role.contains("成长") || role.contains("核心") {
            score += 0.8 * weights.scaling * role_relevance;
        }
        if role.contains("经济") {
            score += 0.8 * weights.economy * role_relevance;
        }
        if role.contains("三连") {
            score += 0.8 * weights.triple * role_relevance;
        }
        if role.contains("战力") || role.contains("过渡") {
            score += 0.8 * weights.tempo * relevance;
        }
    }

    score
}

fn board_keep_score(card: &AgentCard, snapshot: &AgentSnapshot, plan: Option<&RoundPlan>, weights: &DecisionWeights) -> f32 {
    card_score(card, snapshot, plan, None, weights) + if card.is_golden { 4.0 } else { 0.0 }
}

fn explain_card_score(card: &AgentCard, score: f32, round: u32, round_plan: Option<&RoundPlan>, watchlist: Option<&WatchlistResponse>) -> String {
    if let Some((stage, watch)) = watch_for_card(watchlist, round, &card.card_id) {
        let relevance = tier_relevance(round, None, card.tavern_tier);
        return format!(
            "{} · {}（{}表，时序系数 {:.2}，本地评分 {:.1}）",
            watch.role, watch.reason, stage, relevance, score
        );
    }
    if let Some(plan) = round_plan {
        return format!("围绕小目标「{}」的本地评分 {:.1}", plan.primary_goal, score);
    }
    format!("基于即时战力、等级和体系协同的本地评分 {:.1}", score)
}

fn watch_for_card<'a>(
    watchlist: Option<&'a WatchlistResponse>,
    round: u32,
    card_id: &str,
) -> Option<(&'a str, &'a super::model::WatchCard)> {
    let watchlist = watchlist?;
    let stage = match round {
        0..=4 => "early",
        5..=8 => "mid",
        _ => "late",
    };
    watchlist
        .find_card_for_stage(stage, card_id)
        .or_else(|| watchlist.find_card(card_id))
}

fn should_freeze(
    snapshot: &AgentSnapshot,
    plan: Option<&RoundPlan>,
    watchlist: Option<&WatchlistResponse>,
    weights: &DecisionWeights,
    catalog: &CardCatalog,
) -> bool {
    let gold = snapshot.gold.unwrap_or(0).max(0);
    snapshot.shop.iter().any(|card| {
        purchase_cost(card, catalog) > gold
            && card_score(card, snapshot, plan, watchlist, weights) >= 7.5
    })
}

fn nonempty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

/// Deterministic safety plan used when the API is not configured or a combat
/// planning request fails. The live executor therefore never becomes unusable
/// just because the slow planner is unavailable.
pub fn fallback_strategic_plan(snapshot: &AgentSnapshot, target_round: u32, need_trinket_plan: bool) -> super::model::StrategicPlanResponse {
    use super::model::{
        EconomyPolicy, StrategicPlanResponse, TierPolicy, TrinketRolePriority, UpgradePosture,
    };

    let hp = snapshot.hero_effective_health.unwrap_or(30);
    let (goal, weights, rerolls, posture) = if hp <= 8 {
        (
            "优先补即时战力，停止贪成长".to_owned(),
            DecisionWeights { tempo: 2.6, survival: 2.7, synergy: 1.2, scaling: 0.35, economy: 0.35, triple: 0.8 },
            5,
            UpgradePosture::Delay,
        )
    } else if hp <= 15 {
        (
            "稳住场面，同时只拿高确定性的体系组件".to_owned(),
            DecisionWeights { tempo: 2.0, survival: 1.9, synergy: 1.4, scaling: 0.8, economy: 0.7, triple: 1.0 },
            4,
            UpgradePosture::Delay,
        )
    } else {
        (
            "提高体系协同并为中长期成长留资源".to_owned(),
            DecisionWeights { tempo: 1.0, survival: 0.9, synergy: 1.5, scaling: 1.6, economy: 1.2, triple: 1.2 },
            3,
            if snapshot.tavern_tier.unwrap_or(1) < expected_tavern_tier(target_round) {
                UpgradePosture::Prefer
            } else {
                UpgradePosture::Neutral
            },
        )
    };
    let direction = snapshot.dominant_tribe().into_iter().collect::<Vec<_>>();
    let round_plan = RoundPlan {
        target_round,
        primary_goal: goal,
        secondary_goals: vec!["优先完成确定收益动作，再考虑刷新".to_owned()],
        direction,
        economy: EconomyPolicy {
            max_rerolls: rerolls,
            gold_reserve: 0,
            reason: "本地安全计划按生命压力设置刷新预算".to_owned(),
        },
        tier_policy: TierPolicy {
            posture,
            target_tier: Some(expected_tavern_tier(target_round)),
            reason: match posture {
                UpgradePosture::Prefer => "低于回合等级曲线，优先考虑升本",
                UpgradePosture::Neutral => "跟随本地升本曲线与商店机会成本",
                UpgradePosture::Delay => "当前生命压力下延后升本",
            }
            .to_owned(),
        },
        weights,
        replan_conditions: vec![
            "获得体系核心".to_owned(),
            "酒馆等级变化".to_owned(),
            "饰品选择完成".to_owned(),
            "生命进入危险阈值".to_owned(),
        ],
        explanation: "DeepSeek 不可用时的确定性兜底计划".to_owned(),
    };
    let trinket_plan = need_trinket_plan.then(|| TrinketPlan {
        target_round,
        desired_effect: if hp <= 15 { "优先提供即时战力或生存能力" } else { "优先补足当前阵容最缺的成长/协同能力" }.to_owned(),
        role_priorities: if hp <= 15 {
            vec![
                TrinketRolePriority { role: "即时战力".to_owned(), weight: 10.0, reason: "降低下一战死亡风险".to_owned() },
                TrinketRolePriority { role: "生存/保护".to_owned(), weight: 8.5, reason: "提高阵容有效交换能力".to_owned() },
                TrinketRolePriority { role: "体系协同".to_owned(), weight: 6.5, reason: "在不牺牲战力前提下强化方向".to_owned() },
                TrinketRolePriority { role: "纯经济".to_owned(), weight: 2.5, reason: "当前不优先继续贪经济".to_owned() },
            ]
        } else {
            vec![
                TrinketRolePriority { role: "体系协同".to_owned(), weight: 9.5, reason: "让当前方向形成更强闭环".to_owned() },
                TrinketRolePriority { role: "长期成长".to_owned(), weight: 8.0, reason: "生命安全时追求上限".to_owned() },
                TrinketRolePriority { role: "经济".to_owned(), weight: 6.0, reason: "允许用资源换后续选择空间".to_owned() },
                TrinketRolePriority { role: "即时战力".to_owned(), weight: 5.5, reason: "避免成长期间战力断层".to_owned() },
            ]
        },
        avoid_roles: if hp <= 15 { vec!["启动过慢".to_owned(), "纯经济".to_owned()] } else { Vec::new() },
        flexibility_note: "真实饰品出现后按作用标签匹配该优先级，不预猜具体饰品".to_owned(),
    });
    StrategicPlanResponse { round_plan, trinket_plan }
}
