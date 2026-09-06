use hearthcoach_harness::demo::model::{RoundPlan, TrinketPlan};

#[test]
fn planner_models_accept_missing_mechanical_target_round() {
    let plan: RoundPlan = serde_json::from_str(r#"{"primary_goal":"提高战力"}"#).unwrap();
    assert_eq!(plan.target_round, 0);

    let trinket: TrinketPlan = serde_json::from_str(r#"{"desired_effect":"补即时战力"}"#).unwrap();
    assert_eq!(trinket.target_round, 0);
}
