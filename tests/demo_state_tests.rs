use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use hearthcoach_harness::{
    demo::{config::DemoConfig, model::WatchCard, server::DemoState},
    harness::CardCatalog,
};

#[test]
fn demo_phase_explicitly_clears_shop_outside_recruit() {
    let state = DemoState::new(
        DemoConfig::default(),
        PathBuf::from("test_demo_config.json"),
        CardCatalog::default(),
    );
    state.begin_match();
    state.set_phase(3, "Recruit");
    state.set_shop(3, vec!["BG_TEST_CARD".to_owned()]);
    let recruit = state.public_state();
    assert_eq!(recruit.current_phase, "Recruit");
    assert_eq!(recruit.current_shop.len(), 1);

    state.set_phase(3, "Combat");
    let combat = state.public_state();
    assert_eq!(combat.current_phase, "Combat");
    assert!(combat.current_shop.is_empty());
    assert!(combat.shop_hits.is_empty());
}

#[test]
fn delayed_shop_update_cannot_revive_recruit_during_combat() {
    let state = DemoState::new(
        DemoConfig::default(),
        PathBuf::from("test_demo_config.json"),
        CardCatalog::default(),
    );
    state.begin_match();
    state.set_phase(5, "Recruit");
    assert!(state.set_shop_revision(5, 2, vec!["BG_A".to_owned()]));
    let epoch = state.public_state().phase_epoch;

    state.set_phase(5, "Combat");
    assert!(!state.set_shop_revision(5, 3, vec!["BG_STALE".to_owned()]));
    let public = state.public_state();
    assert_eq!(public.current_phase, "Combat");
    assert!(public.current_shop.is_empty());
    assert!(public.phase_epoch > epoch);
}

#[test]
fn watch_card_carries_authoritative_tavern_tier_for_ui() {
    let card = WatchCard {
        card_id: "BG_TTN_401".to_owned(),
        name: "星元自动机".to_owned(),
        tavern_tier: Some(2),
        priority: "S".to_owned(),
        role: "核心成长".to_owned(),
        reason: "测试".to_owned(),
    };
    let value = serde_json::to_value(card).unwrap();
    assert_eq!(value["tavern_tier"], 2);
}

#[test]
fn v04_defaults_enable_agent_and_trinket_planning() {
    let config = DemoConfig::default();
    assert!(!config.open_browser_on_start);
    assert!(config.overlay.enabled);
    assert!(config.agent.enabled);
    assert!(config.agent.auto_plan_during_combat);
    assert_eq!(config.agent.trinket_rounds, vec![6, 9]);
    assert!(config.overlay.shop_slot_spacing_ratio > 0.0);
    assert!(config.overlay.highlight_delay_ms >= 200);
    assert!(config.overlay.card_width_ratio > 0.0);
    assert!(config.overlay.card_height_ratio > 0.0);
    assert!(config.overlay.panel_width_ratio > 0.0);
    assert!(config.overlay.panel_height_ratio > 0.0);
    assert!(config.overlay.panel_x_ratio.is_none());
    assert!(config.overlay.panel_y_ratio.is_none());
}

#[test]
fn overlay_test_mode_can_be_toggled_independently_from_ai_hits() {
    let state = DemoState::new(
        DemoConfig::default(),
        PathBuf::from("test_demo_config.json"),
        CardCatalog::default(),
    );
    state.begin_match();
    state.set_phase(1, "Recruit");
    state.set_shop(1, vec!["BG_TEST_A".to_owned(), "BG_TEST_B".to_owned(), "BG_TEST_C".to_owned()]);
    assert!(!state.public_state().overlay_test_mode);
    assert!(state.toggle_overlay_test_mode());
    assert!(state.public_state().overlay_test_mode);
    assert!(!state.toggle_overlay_test_mode());
}


#[test]
fn new_shop_revision_hides_marks_until_visual_settle() {
    let state = DemoState::new(
        DemoConfig::default(),
        PathBuf::from("test_demo_config.json"),
        CardCatalog::default(),
    );
    state.begin_match();
    state.set_phase(4, "Recruit");
    assert!(state.set_shop_revision(4, 7, vec!["BG_A".to_owned()]));
    let public = state.public_state();
    assert_eq!(public.shop_revision, 7);
    assert_eq!(public.decision_revision, 0);
    assert!(!public.overlay_marks_ready);
}


#[test]
fn overlay_panel_layout_can_be_persisted_and_reset() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "hearthcoach_overlay_layout_{}_{}.json",
        std::process::id(),
        suffix
    ));
    let state = DemoState::new(DemoConfig::default(), path.clone(), CardCatalog::default());

    state
        .update_overlay_panel_layout(0.20, 0.18, 0.31, 0.36)
        .unwrap();
    let saved: DemoConfig = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.overlay.panel_x_ratio, Some(0.20));
    assert_eq!(saved.overlay.panel_y_ratio, Some(0.18));
    assert!((saved.overlay.panel_width_ratio - 0.31).abs() < f32::EPSILON);
    assert!((saved.overlay.panel_height_ratio - 0.36).abs() < f32::EPSILON);

    state.reset_overlay_panel_layout().unwrap();
    let reset: DemoConfig = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(reset.overlay.panel_x_ratio.is_none());
    assert!(reset.overlay.panel_y_ratio.is_none());
    assert_eq!(
        reset.overlay.panel_width_ratio,
        DemoConfig::default().overlay.panel_width_ratio
    );
    assert_eq!(
        reset.overlay.panel_height_ratio,
        DemoConfig::default().overlay.panel_height_ratio
    );

    let _ = fs::remove_file(path);
}

#[test]
fn public_stage_tracks_round_for_automatic_top_guide() {
    let state = DemoState::new(
        DemoConfig::default(),
        PathBuf::from("test_demo_config.json"),
        CardCatalog::default(),
    );
    state.begin_match();

    state.set_phase(4, "Recruit");
    assert_eq!(state.public_state().current_stage, "early");

    state.set_phase(5, "Recruit");
    assert_eq!(state.public_state().current_stage, "mid");

    state.set_phase(9, "Recruit");
    assert_eq!(state.public_state().current_stage, "late");
}
