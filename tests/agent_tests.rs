use std::collections::BTreeSet;

use hearthcoach_harness::{
    demo::{
        agent::{
            detect_replan, evaluate_recruit, expected_tavern_tier, fallback_strategic_plan,
            tier_relevance, AgentCard, AgentSnapshot, DecisionFingerprint,
        },
        model::{
            DecisionWeights, ReplanLevel, StageWatchlist, UpgradePosture, WatchCard,
            WatchlistResponse,
        },
    },
    harness::CardCatalog,
};

#[test]
fn decision_weights_are_clamped_to_safe_range() {
    let weights = DecisionWeights {
        tempo: 99.0,
        scaling: -4.0,
        economy: f32::NAN,
        synergy: 2.2,
        triple: 1.0,
        survival: 3.5,
    }
    .normalized();
    assert_eq!(weights.tempo, 3.0);
    assert_eq!(weights.scaling, 0.0);
    assert_eq!(weights.economy, 1.0);
    assert_eq!(weights.survival, 3.0);
}

#[test]
fn major_state_changes_raise_strategic_replan_events() {
    let before = DecisionFingerprint {
        effective_health: Some(22),
        tavern_tier: Some(3),
        trinket_count: 0,
        golden_count: 0,
        dominant_tribe: Some("MECH".to_owned()),
        owned_card_ids: BTreeSet::new(),
    };
    let mut owned = BTreeSet::new();
    owned.insert("BG_CORE".to_owned());
    let now = DecisionFingerprint {
        effective_health: Some(7),
        tavern_tier: Some(4),
        trinket_count: 1,
        golden_count: 1,
        dominant_tribe: Some("MECH".to_owned()),
        owned_card_ids: owned,
    };
    let events = detect_replan(&before, &now, &["BG_CORE".to_owned()], 15, 8);
    assert!(events.iter().any(|event| event.code == "tavern_tier_changed"));
    assert!(events.iter().any(|event| event.code == "trinket_acquired"));
    assert!(events.iter().any(|event| event.code == "core_card_acquired"));
    assert!(events.iter().any(|event| event.code == "triple_completed"));
    assert!(events.iter().any(|event| event.level == ReplanLevel::LongTerm));
}

#[test]
fn low_health_fallback_plan_prioritizes_survival_and_trinket_tempo() {
    let snapshot = AgentSnapshot {
        round_number: 5,
        phase: "Combat".to_owned(),
        hero_effective_health: Some(7),
        ..Default::default()
    };
    let response = fallback_strategic_plan(&snapshot, 6, true);
    assert_eq!(response.round_plan.target_round, 6);
    assert_eq!(response.round_plan.tier_policy.posture, UpgradePosture::Delay);
    assert!(response.round_plan.weights.survival > response.round_plan.weights.scaling);
    let trinket = response.trinket_plan.unwrap();
    assert_eq!(trinket.target_round, 6);
    assert_eq!(trinket.role_priorities.first().unwrap().role, "即时战力");
}


#[test]
fn low_tier_priority_decays_with_round_curve() {
    assert_eq!(expected_tavern_tier(5), 3);
    assert_eq!(tier_relevance(5, Some(2), Some(1)), 0.5);
    assert_eq!(tier_relevance(5, Some(2), Some(3)), 1.0);
}

#[test]
fn round_five_t3_a_can_outrank_old_t1_s() {
    let t1 = AgentCard {
        entity_id: 1,
        card_id: "T1_S".to_owned(),
        name: "前期一本到S牌".to_owned(),
        attack: Some(3),
        health: Some(3),
        tavern_tier: Some(1),
        ..Default::default()
    };
    let t3 = AgentCard {
        entity_id: 2,
        card_id: "T3_A".to_owned(),
        name: "当前三本A牌".to_owned(),
        attack: Some(4),
        health: Some(4),
        tavern_tier: Some(3),
        ..Default::default()
    };
    let snapshot = AgentSnapshot {
        round_number: 5,
        phase: "Recruit".to_owned(),
        hero_effective_health: Some(30),
        gold: Some(10),
        tavern_tier: Some(3),
        refresh_cost: Some(1),
        upgrade_cost: Some(6),
        shop: vec![t1, t3],
        ..Default::default()
    };
    let watchlist = WatchlistResponse {
        stages: vec![
            StageWatchlist {
                stage: "early".to_owned(),
                cards: vec![WatchCard {
                    card_id: "T1_S".to_owned(),
                    name: "前期一本到S牌".to_owned(),
                    tavern_tier: Some(1),
                    priority: "S".to_owned(),
                    role: "过渡战力".to_owned(),
                    reason: "前期强".to_owned(),
                }],
            },
            StageWatchlist {
                stage: "mid".to_owned(),
                cards: vec![WatchCard {
                    card_id: "T3_A".to_owned(),
                    name: "当前三本A牌".to_owned(),
                    tavern_tier: Some(3),
                    priority: "A".to_owned(),
                    role: "核心成长".to_owned(),
                    reason: "当前阶段核心".to_owned(),
                }],
            },
        ],
        ..Default::default()
    };
    let tactical = evaluate_recruit(
        &snapshot,
        None,
        None,
        Some(&watchlist),
        &CardCatalog::default(),
        0,
        1,
    );
    let t1_score = tactical
        .ranked_actions
        .iter()
        .find(|action| action.card_id.as_deref() == Some("T1_S"))
        .unwrap()
        .score;
    let t3_score = tactical
        .ranked_actions
        .iter()
        .find(|action| action.card_id.as_deref() == Some("T3_A"))
        .unwrap()
        .score;
    assert!(t3_score > t1_score, "T3 A={t3_score}, old T1 S={t1_score}");
}

#[test]
fn being_behind_curve_pushes_upgrade_up_the_action_ranking() {
    let snapshot = AgentSnapshot {
        round_number: 5,
        phase: "Recruit".to_owned(),
        hero_effective_health: Some(30),
        gold: Some(8),
        tavern_tier: Some(2),
        refresh_cost: Some(1),
        upgrade_cost: Some(5),
        shop: Vec::new(),
        ..Default::default()
    };
    let tactical = evaluate_recruit(
        &snapshot,
        None,
        None,
        None,
        &CardCatalog::default(),
        0,
        3,
    );
    assert_eq!(tactical.ranked_actions.first().unwrap().kind, "upgrade");
}

#[test]
fn upgrade_can_softly_spend_into_gold_reserve_when_behind_curve() {
    let snapshot = AgentSnapshot {
        round_number: 5,
        phase: "Recruit".to_owned(),
        hero_effective_health: Some(30),
        gold: Some(8),
        tavern_tier: Some(2),
        refresh_cost: Some(1),
        upgrade_cost: Some(5),
        shop: Vec::new(),
        ..Default::default()
    };
    let mut plan = fallback_strategic_plan(&snapshot, 5, false).round_plan;
    plan.economy.gold_reserve = 4; // spendable would be only 4 under a hard reserve.
    let tactical = evaluate_recruit(
        &snapshot,
        Some(&plan),
        None,
        None,
        &CardCatalog::default(),
        0,
        4,
    );
    assert!(tactical
        .ranked_actions
        .iter()
        .any(|action| action.kind == "upgrade"));
    assert_eq!(tactical.ranked_actions.first().unwrap().kind, "upgrade");
}
