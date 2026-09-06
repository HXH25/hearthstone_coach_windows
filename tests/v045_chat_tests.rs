use hearthcoach_harness::demo::model::{
    DecisionWeights, EconomyPolicy, RoundPlan, RoundPlanPatch, TierPolicy, UpgradePosture,
};

#[test]
fn chat_patch_changes_only_requested_fields_and_keeps_target_round() {
    let mut plan = RoundPlan {
        target_round: 7,
        primary_goal: "补即时战力".to_owned(),
        secondary_goals: vec!["保对子".to_owned()],
        direction: vec!["MECH".to_owned()],
        economy: EconomyPolicy {
            max_rerolls: 3,
            gold_reserve: 1,
            reason: "保一点资源".to_owned(),
        },
        tier_policy: TierPolicy {
            posture: UpgradePosture::Neutral,
            target_tier: Some(4),
            reason: "看商店".to_owned(),
        },
        weights: DecisionWeights::default(),
        replan_conditions: vec!["低血".to_owned()],
        explanation: "原计划".to_owned(),
    };

    let patch: RoundPlanPatch = serde_json::from_str(
        r#"{
          "primary_goal":"优先升本，同时保留机械核心",
          "tier_policy":{"posture":"prefer","target_tier":5},
          "weights":{"scaling":2.1,"tempo":0.8}
        }"#,
    )
    .unwrap();

    assert!(patch.apply_to(&mut plan));
    assert_eq!(plan.target_round, 7);
    assert_eq!(plan.primary_goal, "优先升本，同时保留机械核心");
    assert_eq!(plan.tier_policy.posture, UpgradePosture::Prefer);
    assert_eq!(plan.tier_policy.target_tier, Some(5));
    assert_eq!(plan.economy.gold_reserve, 1);
    assert_eq!(plan.direction, vec!["MECH".to_owned()]);
    assert!((plan.weights.scaling - 2.1).abs() < f32::EPSILON);
    assert!((plan.weights.tempo - 0.8).abs() < f32::EPSILON);
}

#[test]
fn empty_chat_patch_is_detected_and_out_of_range_values_are_clamped() {
    let empty: RoundPlanPatch = serde_json::from_str("{}").unwrap();
    assert!(empty.is_empty());

    let mut plan = RoundPlan::default();
    plan.target_round = 9;
    let patch: RoundPlanPatch = serde_json::from_str(
        r#"{
          "economy":{"max_rerolls":99,"gold_reserve":-5},
          "tier_policy":{"target_tier":99},
          "weights":{"survival":9.0}
        }"#,
    )
    .unwrap();
    assert!(patch.apply_to(&mut plan));
    assert_eq!(plan.target_round, 9);
    assert_eq!(plan.economy.max_rerolls, 8);
    assert_eq!(plan.economy.gold_reserve, 0);
    assert_eq!(plan.tier_policy.target_tier, Some(6));
    assert!((plan.weights.survival - 3.0).abs() < f32::EPSILON);
}
