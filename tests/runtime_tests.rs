use hearthcoach_harness::harness::{
    CardCatalog, CardMeta, ChoiceKind, CombatResult, HarnessEvent, HarnessRuntime,
    RecruitActionKind,
};

fn gs(payload: &str) -> String {
    format!("D 10:00:00.0000000 GameState.DebugPrintPower() - {payload}")
}

fn feed(runtime: &mut HarnessRuntime, payload: &str) -> Vec<HarnessEvent> {
    runtime.feed_line(&gs(payload))
}

fn feed_name(runtime: &mut HarnessRuntime, player_id: i32, name: &str) {
    runtime.feed_line(&format!(
        "D 10:00:00.0000000 GameState.DebugPrintGame() - PlayerID={player_id}, PlayerName={name}"
    ));
}

fn send_option(runtime: &mut HarnessRuntime, selected_option: i32, selected_target: u32, selected_position: i32) {
    runtime.feed_line(&format!(
        "D 10:00:00.0000000 GameState.SendOption() - selectedOption={selected_option} selectedSubOption=-1 selectedTarget={selected_target} selectedPosition={selected_position}"
    ));
}

fn create_entity(runtime: &mut HarnessRuntime, id: u32, card_id: &str, tags: &[(&str, &str)]) {
    feed(
        runtime,
        &format!("FULL_ENTITY - Creating ID={id} CardID={card_id}"),
    );
    for (tag, value) in tags {
        feed(runtime, &format!("    tag={tag} value={value}"));
    }
}

fn base_runtime() -> HarnessRuntime {
    let catalog = CardCatalog::from_cards([
        CardMeta {
            card_id: "BG_TTN_401".to_owned(),
            name_zh_cn: Some("星元自动机".to_owned()),
            card_type: Some("MINION".to_owned()),
            race: Some("MECHANICAL".to_owned()),
            ..Default::default()
        },
        CardMeta {
            card_id: "BG_TEST_SPELL".to_owned(),
            name_zh_cn: Some("测试酒馆法术".to_owned()),
            card_type: Some("BATTLEGROUND_SPELL".to_owned()),
            ..Default::default()
        },
        CardMeta {
            card_id: "BG_ACTIVATE".to_owned(),
            name_zh_cn: Some("测试激活随从".to_owned()),
            card_type: Some("MINION".to_owned()),
            race: Some("BEAST".to_owned()),
            activate_keyword: true,
            ..Default::default()
        },
        CardMeta {
            card_id: "BG_PLAIN".to_owned(),
            name_zh_cn: Some("普通随从".to_owned()),
            card_type: Some("MINION".to_owned()),
            race: Some("BEAST".to_owned()),
            ..Default::default()
        },
        CardMeta {
            card_id: "MY_HERO".to_owned(),
            name_zh_cn: Some("我方英雄".to_owned()),
            card_type: Some("HERO".to_owned()),
            ..Default::default()
        },
        CardMeta {
            card_id: "OPP_HERO".to_owned(),
            name_zh_cn: Some("对手英雄".to_owned()),
            card_type: Some("HERO".to_owned()),
            ..Default::default()
        },
    ]);
    let mut r = HarnessRuntime::with_catalog(Some("Power.log".to_owned()), catalog);
    feed(&mut r, "CREATE_GAME");
    feed(&mut r, "GameEntity EntityID=10");
    feed(
        &mut r,
        "Player EntityID=11 PlayerID=4 GameAccountId=[hi=1 lo=2]",
    );
    feed_name(&mut r, 4, "Me");
    feed(&mut r, "    tag=NEXT_OPPONENT_PLAYER_ID value=3");
    feed(&mut r, "    tag=PLAYER_TECH_LEVEL value=1");
    feed(
        &mut r,
        "Player EntityID=12 PlayerID=12 GameAccountId=[hi=0 lo=0]",
    );

    create_entity(
        &mut r,
        100,
        "MY_HERO",
        &[
            ("CONTROLLER", "4"),
            ("CARDTYPE", "HERO"),
            ("PLAYER_ID", "4"),
            ("HEALTH", "30"),
            ("ARMOR", "0"),
            ("ZONE", "PLAY"),
        ],
    );
    create_entity(
        &mut r,
        200,
        "OPP_HERO",
        &[
            ("CONTROLLER", "12"),
            ("CARDTYPE", "HERO"),
            ("PLAYER_ID", "3"),
            ("HEALTH", "30"),
            ("ARMOR", "0"),
            ("ZONE", "PLAY"),
        ],
    );
    r
}

#[test]
fn block_buy_is_archived_as_buy_minion() {
    let mut r = base_runtime();
    create_entity(
        &mut r,
        419,
        "BG_TTN_401",
        &[
            ("CONTROLLER", "12"),
            ("CARDTYPE", "MINION"),
            ("ZONE", "PLAY"),
            ("ZONE_POSITION", "1"),
            ("HAS_DRAG_TO_BUY", "1"),
        ],
    );

    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=1");
    feed(
        &mut r,
        "TAG_CHANGE Entity=GameEntity tag=STEP value=MAIN_ACTION",
    );
    send_option(&mut r, 1, 419, 0);
    feed(
        &mut r,
        "BLOCK_START BlockType=PLAY Entity=[entityName=购买 id=80 zone=PLAY zonePos=0 cardId=TB_BaconShop_DragBuy player=4] EffectCardId= EffectIndex=0 Target=[entityName=星元自动机 id=419 zone=PLAY zonePos=1 cardId=BG_TTN_401 player=12] SubOption=-1",
    );
    feed(
        &mut r,
        "TAG_CHANGE Entity=[entityName=星元自动机 id=419 zone=PLAY zonePos=1 cardId=BG_TTN_401 player=12] tag=CONTROLLER value=4",
    );
    feed(
        &mut r,
        "TAG_CHANGE Entity=[entityName=星元自动机 id=419 zone=PLAY zonePos=1 cardId=BG_TTN_401 player=12] tag=ZONE value=HAND",
    );
    feed(&mut r, "BLOCK_END");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=2");

    let recruit = r.archive().round(1).and_then(|round| round.recruit.as_ref());
    // Recruit is pending until Combat is finalized, so query the live/current archive via turn 2 finish.
    assert!(recruit.is_none());
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=3");
    let recruit = r.archive().round(1).unwrap().recruit.as_ref().unwrap();
    assert_eq!(recruit.actions.len(), 1);
    assert_eq!(recruit.actions[0].kind, RecruitActionKind::BuyMinion);
    assert_eq!(
        recruit.actions[0]
            .target
            .as_ref()
            .and_then(|card| card.name.as_deref()),
        Some("星元自动机")
    );
}

#[test]
fn shop_revisions_preserve_last_shop_across_cleanup() {
    let mut r = base_runtime();
    create_entity(
        &mut r,
        401,
        "BG_TTN_401",
        &[
            ("CONTROLLER", "12"),
            ("CARDTYPE", "MINION"),
            ("ZONE", "PLAY"),
            ("ZONE_POSITION", "1"),
            ("HAS_DRAG_TO_BUY", "1"),
        ],
    );
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=1");
    feed(
        &mut r,
        "TAG_CHANGE Entity=GameEntity tag=STEP value=MAIN_ACTION",
    );

    send_option(&mut r, 1, 0, 0);
    feed(
        &mut r,
        "BLOCK_START BlockType=PLAY Entity=[entityName=刷新 id=81 zone=PLAY zonePos=0 cardId=TB_BaconShop_8p_Reroll_Button player=4] EffectCardId= EffectIndex=0 Target=0 SubOption=-1",
    );
    feed(
        &mut r,
        "TAG_CHANGE Entity=[entityName=X id=401 zone=PLAY zonePos=1 cardId=BG_TTN_401 player=12] tag=ZONE value=REMOVEDFROMGAME",
    );
    create_entity(
        &mut r,
        402,
        "BG_TTN_401",
        &[
            ("CONTROLLER", "12"),
            ("CARDTYPE", "MINION"),
            ("ZONE", "PLAY"),
            ("ZONE_POSITION", "1"),
            ("HAS_DRAG_TO_BUY", "1"),
        ],
    );
    feed(&mut r, "BLOCK_END");
    // A deterministic replay flush represents the stable period after the refresh.
    r.flush_pending_shop_revision();

    // Simulate the pre-combat cleanup that caused V0.2 End Shop = empty.
    feed(
        &mut r,
        "TAG_CHANGE Entity=[entityName=X id=402 zone=PLAY zonePos=1 cardId=BG_TTN_401 player=12] tag=ZONE value=REMOVEDFROMGAME",
    );
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=2");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=3");

    let recruit = r.archive().round(1).unwrap().recruit.as_ref().unwrap();
    assert!(recruit.shop_revisions.len() >= 2);
    assert_eq!(recruit.end.shop.len(), 1);
    assert_eq!(recruit.end.shop[0].card_id.as_deref(), Some("BG_TTN_401"));
}

#[test]
fn explicit_battleground_tags_record_win_and_damage_dealt() {
    let mut r = base_runtime();
    create_entity(
        &mut r,
        300,
        "BG_TTN_401",
        &[
            ("CONTROLLER", "4"),
            ("CARDTYPE", "MINION"),
            ("ZONE", "PLAY"),
            ("ZONE_POSITION", "1"),
            ("ATK", "3"),
            ("HEALTH", "4"),
        ],
    );
    create_entity(
        &mut r,
        301,
        "BG_TTN_401",
        &[
            ("CONTROLLER", "12"),
            ("CARDTYPE", "MINION"),
            ("ZONE", "PLAY"),
            ("ZONE_POSITION", "1"),
            ("ATK", "1"),
            ("HEALTH", "1"),
        ],
    );

    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=1");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=2");
    feed(
        &mut r,
        "BLOCK_START BlockType=ATTACK Entity=[entityName=Me id=300 zone=PLAY zonePos=1 cardId=BG_TTN_401 player=4] EffectIndex=0 Target=0 SubOption=-1",
    );
    feed(
        &mut r,
        "TAG_CHANGE Entity=[entityName=Opp id=200 zone=PLAY zonePos=0 cardId=OPP_HERO player=12] tag=PREDAMAGE value=8",
    );
    feed(
        &mut r,
        "TAG_CHANGE Entity=Me tag=BACON_WON_LAST_COMBAT value=1",
    );
    feed(
        &mut r,
        "TAG_CHANGE Entity=Me tag=DAMAGE_DEALT_TO_HERO_LAST_TURN value=0",
    );
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=3");

    let combat = r.archive().round(1).unwrap().combat.as_ref().unwrap();
    assert_eq!(combat.result, CombatResult::Win);
    assert_eq!(combat.damage_taken, Some(0));
    assert_eq!(combat.damage_dealt, Some(8));
}

#[test]
fn explicit_battleground_tags_record_loss() {
    let mut r = base_runtime();
    create_entity(
        &mut r,
        300,
        "BG_TTN_401",
        &[
            ("CONTROLLER", "4"),
            ("CARDTYPE", "MINION"),
            ("ZONE", "PLAY"),
            ("ZONE_POSITION", "1"),
        ],
    );
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=1");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=2");
    feed(
        &mut r,
        "BLOCK_START BlockType=ATTACK Entity=[entityName=Me id=300 zone=PLAY zonePos=1 cardId=BG_TTN_401 player=4] EffectIndex=0 Target=0 SubOption=-1",
    );
    feed(
        &mut r,
        "TAG_CHANGE Entity=Me tag=BACON_WON_LAST_COMBAT value=0",
    );
    feed(
        &mut r,
        "TAG_CHANGE Entity=Me tag=DAMAGE_DEALT_TO_HERO_LAST_TURN value=5",
    );
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=3");

    let combat = r.archive().round(1).unwrap().combat.as_ref().unwrap();
    assert_eq!(combat.result, CombatResult::Loss);
    assert_eq!(combat.damage_taken, Some(5));
}

#[test]
fn live_query_returns_shop_only() {
    let mut r = base_runtime();
    create_entity(
        &mut r,
        400,
        "BG_TTN_401",
        &[
            ("CONTROLLER", "12"),
            ("CARDTYPE", "MINION"),
            ("ZONE", "PLAY"),
            ("ZONE_POSITION", "1"),
            ("HAS_DRAG_TO_BUY", "1"),
        ],
    );
    assert_eq!(r.current_shop().len(), 1);
    assert_eq!(r.current_shop()[0].card_id.as_deref(), Some("BG_TTN_401"));
    assert_eq!(r.current_shop()[0].name.as_deref(), Some("星元自动机"));
}


#[test]
fn buy_spell_and_sell_are_archived_from_button_source_and_target() {
    let mut r = base_runtime();
    create_entity(
        &mut r,
        510,
        "BG_TEST_SPELL",
        &[
            ("CONTROLLER", "12"),
            ("CARDTYPE", "BATTLEGROUND_SPELL"),
            ("ZONE", "PLAY"),
            ("ZONE_POSITION", "1"),
            ("HAS_DRAG_TO_BUY", "1"),
        ],
    );
    create_entity(
        &mut r,
        511,
        "BG_TTN_401",
        &[
            ("CONTROLLER", "4"),
            ("CARDTYPE", "MINION"),
            ("ZONE", "PLAY"),
            ("ZONE_POSITION", "1"),
        ],
    );

    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=1");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=STEP value=MAIN_ACTION");

    send_option(&mut r, 1, 510, 0);
    feed(
        &mut r,
        "BLOCK_START BlockType=PLAY Entity=[entityName=拖动即可购买法术 id=425 zone=PLAY zonePos=0 cardId=TB_BaconShop_DragBuy_Spell player=4] EffectCardId=System.Collections.Generic.List`1[System.String] EffectIndex=0 Target=[entityName=测试酒馆法术 id=510 zone=PLAY zonePos=1 cardId=BG_TEST_SPELL player=12] SubOption=-1",
    );
    feed(
        &mut r,
        "TAG_CHANGE Entity=[entityName=测试酒馆法术 id=510 zone=PLAY zonePos=1 cardId=BG_TEST_SPELL player=12] tag=CONTROLLER value=4",
    );
    feed(
        &mut r,
        "TAG_CHANGE Entity=[entityName=测试酒馆法术 id=510 zone=PLAY zonePos=1 cardId=BG_TEST_SPELL player=12] tag=ZONE value=HAND",
    );
    feed(&mut r, "BLOCK_END");

    send_option(&mut r, 2, 511, 0);
    feed(
        &mut r,
        "BLOCK_START BlockType=PLAY Entity=[entityName=拖动随从将其出售 id=407 zone=PLAY zonePos=0 cardId=TB_BaconShop_DragSell player=4] EffectCardId=System.Collections.Generic.List`1[System.String] EffectIndex=0 Target=[entityName=星元自动机 id=511 zone=PLAY zonePos=1 cardId=BG_TTN_401 player=4] SubOption=-1",
    );
    feed(
        &mut r,
        "TAG_CHANGE Entity=[entityName=星元自动机 id=511 zone=PLAY zonePos=1 cardId=BG_TTN_401 player=4] tag=ZONE value=REMOVEDFROMGAME",
    );
    feed(&mut r, "BLOCK_END");

    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=2");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=3");

    let recruit = r.archive().round(1).unwrap().recruit.as_ref().unwrap();
    assert_eq!(recruit.actions.len(), 2);
    assert_eq!(recruit.actions[0].kind, RecruitActionKind::BuySpell);
    assert_eq!(recruit.actions[0].target.as_ref().and_then(|c| c.card_id.as_deref()), Some("BG_TEST_SPELL"));
    assert_eq!(recruit.actions[1].kind, RecruitActionKind::SellMinion);
    assert_eq!(recruit.actions[1].target.as_ref().and_then(|c| c.card_id.as_deref()), Some("BG_TTN_401"));
}

#[test]
fn loss_is_detected_when_only_damage_tag_is_present() {
    let mut r = base_runtime();
    create_entity(
        &mut r,
        300,
        "BG_TTN_401",
        &[
            ("CONTROLLER", "4"),
            ("CARDTYPE", "MINION"),
            ("ZONE", "PLAY"),
            ("ZONE_POSITION", "1"),
        ],
    );
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=1");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=2");
    feed(
        &mut r,
        "BLOCK_START BlockType=ATTACK Entity=[entityName=Me id=300 zone=PLAY zonePos=1 cardId=BG_TTN_401 player=4] EffectIndex=0 Target=0 SubOption=-1",
    );
    feed(
        &mut r,
        "TAG_CHANGE Entity=Me tag=DAMAGE_DEALT_TO_HERO_LAST_TURN value=5",
    );
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=3");

    let combat = r.archive().round(1).unwrap().combat.as_ref().unwrap();
    assert_eq!(combat.result, CombatResult::Loss);
    assert_eq!(combat.damage_taken, Some(5));
}

#[test]
fn recruit_start_waits_for_new_turn_resource_reset() {
    let mut r = base_runtime();
    // Simulate previous-turn spent resources still present when TURN flips.
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES value=3");
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES_USED value=3");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=1");

    // New maximum resources arrives first; this must NOT snapshot gold=0 yet.
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES value=5");
    // Reset used resources is the readiness point.
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES_USED value=0");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=STEP value=MAIN_ACTION");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=2");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=3");

    let recruit = r.archive().round(1).unwrap().recruit.as_ref().unwrap();
    assert_eq!(recruit.start.gold, Some(5));
}

#[test]
fn refresh_shop_is_not_exposed_until_full_stable_flush() {
    let mut r = base_runtime();
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES value=5");
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES_USED value=0");
    create_entity(
        &mut r,
        600,
        "BG_TTN_401",
        &[("CONTROLLER", "12"), ("CARDTYPE", "MINION"), ("ZONE", "PLAY"), ("ZONE_POSITION", "1"), ("HAS_DRAG_TO_BUY", "1")],
    );
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=1");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=STEP value=MAIN_ACTION");

    // Force the initial stable shop.
    let initial_events = r.flush_pending_shop_revision();
    assert!(initial_events.iter().any(|event| matches!(event, HarnessEvent::ShopUpdated { .. })));
    assert_eq!(r.current_shop_card_ids().len(), 1);

    send_option(&mut r, 1, 0, 0);
    feed(
        &mut r,
        "BLOCK_START BlockType=PLAY Entity=[entityName=刷新 id=81 zone=PLAY zonePos=0 cardId=TB_BaconShop_8p_Reroll_Button player=4] EffectCardId= EffectIndex=0 Target=0 SubOption=-1",
    );
    feed(&mut r, "TAG_CHANGE Entity=[entityName=X id=600 zone=PLAY zonePos=1 cardId=BG_TTN_401 player=12] tag=ZONE value=REMOVEDFROMGAME");
    // Two entities arrive before BLOCK_END...
    for (id, pos) in [(601, "1"), (602, "2")] {
        create_entity(&mut r, id, "BG_TTN_401", &[("CONTROLLER", "12"), ("CARDTYPE", "MINION"), ("ZONE", "PLAY"), ("ZONE_POSITION", pos)]);
    }
    feed(&mut r, "BLOCK_END");
    // ...and three more arrive asynchronously afterwards.
    for (id, pos) in [(603, "3"), (604, "4"), (605, "5")] {
        create_entity(&mut r, id, "BG_TTN_401", &[("CONTROLLER", "12"), ("CARDTYPE", "MINION"), ("ZONE", "PLAY"), ("ZONE_POSITION", pos)]);
    }

    // Dirty shops deliberately return no fast-path IDs.
    assert!(r.current_shop_card_ids().is_empty());
    let events = r.flush_pending_shop_revision();
    let ids = events.iter().find_map(|event| match event {
        HarnessEvent::ShopUpdated { card_ids, .. } => Some(card_ids.clone()),
        _ => None,
    }).unwrap();
    assert_eq!(ids.len(), 5);
    assert_eq!(r.current_shop_card_ids().len(), 5);
    let revision = r.current_shop_revision().unwrap();
    assert_eq!(revision.cards.len(), 5);
}

#[test]
fn recruit_snapshot_exposes_costs_freeze_timeout_and_hero_power() {
    let mut r = base_runtime();
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES value=5");
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES_USED value=0");
    feed(&mut r, "TAG_CHANGE Entity=11 tag=TIMEOUT value=75");

    create_entity(&mut r, 700, "TB_BaconShop_8p_Reroll_Button", &[("CONTROLLER", "4"), ("CARDTYPE", "SPELL"), ("ZONE", "PLAY"), ("COST", "1")]);
    create_entity(&mut r, 701, "TB_BaconShopTechUp02_Button", &[("CONTROLLER", "4"), ("CARDTYPE", "SPELL"), ("ZONE", "PLAY"), ("TECH_LEVEL", "2"), ("COST", "4")]);
    create_entity(&mut r, 702, "TB_BaconShopLockAll_Button", &[("CONTROLLER", "4"), ("CARDTYPE", "SPELL"), ("ZONE", "PLAY"), ("TAG_SCRIPT_DATA_NUM_1", "1")]);
    create_entity(&mut r, 703, "MY_POWER", &[("CONTROLLER", "4"), ("CARDTYPE", "HERO_POWER"), ("ZONE", "PLAY"), ("COST", "0"), ("EXHAUSTED", "0"), ("TAG_SCRIPT_DATA_NUM_1", "2")]);

    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=1");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=STEP value=MAIN_ACTION");
    let snapshot = r.current_recruit_state().unwrap();
    assert_eq!(snapshot.gold, Some(5));
    assert_eq!(snapshot.refresh_cost, Some(1));
    assert_eq!(snapshot.upgrade_cost, Some(4));
    assert_eq!(snapshot.shop_frozen, Some(true));
    assert_eq!(snapshot.timeout_seconds, Some(75));
    let power = snapshot.hero_power.unwrap();
    assert_eq!(power.card_id.as_deref(), Some("MY_POWER"));
    assert_eq!(power.cost, Some(0));
    assert!(power.available);
    assert_eq!(power.dynamic_tags.get("TAG_SCRIPT_DATA_NUM_1").map(String::as_str), Some("2"));
}

#[test]
fn reorder_and_recruit_attack_are_semantic_actions() {
    let mut r = base_runtime();
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES value=5");
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES_USED value=0");
    create_entity(&mut r, 800, "BG_TTN_401", &[("CONTROLLER", "4"), ("CARDTYPE", "MINION"), ("ZONE", "PLAY"), ("ZONE_POSITION", "1")]);
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=1");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=STEP value=MAIN_ACTION");

    send_option(&mut r, 1, 800, 2);
    feed(&mut r, "BLOCK_START BlockType=MOVE_MINION Entity=[entityName=星元自动机 id=800 zone=PLAY zonePos=1 cardId=BG_TTN_401 player=4] EffectCardId= EffectIndex=0 Target=0 SubOption=-1");
    feed(&mut r, "TAG_CHANGE Entity=800 tag=ZONE_POSITION value=2");
    feed(&mut r, "BLOCK_END");
    send_option(&mut r, 2, 0, 0);
    feed(&mut r, "BLOCK_START BlockType=ATTACK Entity=[entityName=星元自动机 id=800 zone=PLAY zonePos=2 cardId=BG_TTN_401 player=4] EffectCardId= EffectIndex=0 Target=0 SubOption=-1");
    feed(&mut r, "BLOCK_END");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=2");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=3");

    let recruit = r.archive().round(1).unwrap().recruit.as_ref().unwrap();
    assert!(recruit.actions.iter().any(|action| action.kind == RecruitActionKind::Reorder));
    assert!(recruit.actions.iter().any(|action| action.kind == RecruitActionKind::RecruitAttack));
}


#[test]
fn shop_uses_hdt_style_board_membership_without_drag_tag() {
    let mut r = base_runtime();
    create_entity(
        &mut r,
        900,
        "BG_TTN_401",
        &[
            ("CONTROLLER", "12"),
            ("CARDTYPE", "MINION"),
            ("ZONE", "PLAY"),
            ("ZONE_POSITION", "1"),
        ],
    );
    create_entity(
        &mut r,
        901,
        "BG_TEST_SPELL",
        &[
            ("CONTROLLER", "12"),
            ("CARDTYPE", "BATTLEGROUND_SPELL"),
            ("ZONE", "PLAY"),
            ("ZONE_POSITION", "2"),
        ],
    );

    let shop = r.current_shop();
    assert_eq!(shop.len(), 2);
    assert_eq!(shop[0].card_id.as_deref(), Some("BG_TTN_401"));
    assert_eq!(shop[1].card_id.as_deref(), Some("BG_TEST_SPELL"));
}

#[test]
fn stale_untracked_block_is_recovered_before_next_user_action() {
    let mut r = base_runtime();
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES value=5");
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES_USED value=0");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=1");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=STEP value=MAIN_ACTION");

    // Simulate a special effect that opens a block our generic parser never
    // sees closed. It has no semantic recruit action.
    feed(
        &mut r,
        "BLOCK_START BlockType=TRIGGER Entity=[entityName=effect id=999 zone=SETASIDE zonePos=0 cardId= player=4] EffectCardId= EffectIndex=0 Target=0 SubOption=-1",
    );

    // A known top-level button action must self-heal the stale untracked frame.
    send_option(&mut r, 1, 0, 0);
    feed(
        &mut r,
        "BLOCK_START BlockType=PLAY Entity=[entityName=刷新 id=81 zone=PLAY zonePos=0 cardId=TB_BaconShop_8p_Reroll_Button player=4] EffectCardId= EffectIndex=0 Target=0 SubOption=-1",
    );
    feed(&mut r, "BLOCK_END");

    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=2");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=3");

    let recruit = r.archive().round(1).unwrap().recruit.as_ref().unwrap();
    assert!(recruit
        .actions
        .iter()
        .any(|action| action.kind == RecruitActionKind::Refresh));
    assert!(r.stale_block_stack_recoveries() >= 1);
}

#[test]
fn turn_boundary_clears_stale_block_stack_for_long_matches() {
    let mut r = base_runtime();
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES value=3");
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES_USED value=0");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=1");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=STEP value=MAIN_ACTION");
    feed(
        &mut r,
        "BLOCK_START BlockType=TRIGGER Entity=[entityName=effect id=998 zone=SETASIDE zonePos=0 cardId= player=4] EffectCardId= EffectIndex=0 Target=0 SubOption=-1",
    );

    // Transition despite the missing BLOCK_END.
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=2");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=3");
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES value=5");
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES_USED value=0");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=STEP value=MAIN_ACTION");
    send_option(&mut r, 1, 0, 0);
    feed(
        &mut r,
        "BLOCK_START BlockType=PLAY Entity=[entityName=刷新 id=81 zone=PLAY zonePos=0 cardId=TB_BaconShop_8p_Reroll_Button player=4] EffectCardId= EffectIndex=0 Target=0 SubOption=-1",
    );
    feed(&mut r, "BLOCK_END");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=4");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=5");

    let recruit = r.archive().round(2).unwrap().recruit.as_ref().unwrap();
    assert!(recruit
        .actions
        .iter()
        .any(|action| action.kind == RecruitActionKind::Refresh));
    assert!(r.stale_block_stack_recoveries() >= 1);
}

#[test]
fn freeze_toggle_is_tracked_as_explicit_state_machine() {
    let mut r = base_runtime();
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES value=3");
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES_USED value=0");
    create_entity(
        &mut r,
        910,
        "TB_BaconShopLockAll_Button",
        &[("CONTROLLER", "4"), ("CARDTYPE", "SPELL"), ("ZONE", "PLAY")],
    );
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=1");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=STEP value=MAIN_ACTION");
    assert_eq!(r.is_shop_frozen(), Some(false));

    send_option(&mut r, 1, 0, 0);
    feed(
        &mut r,
        "BLOCK_START BlockType=PLAY Entity=[entityName=冻结 id=910 zone=PLAY zonePos=0 cardId=TB_BaconShopLockAll_Button player=4] EffectCardId= EffectIndex=0 Target=0 SubOption=-1",
    );
    feed(&mut r, "BLOCK_END");
    assert_eq!(r.is_shop_frozen(), Some(true));
    assert_eq!(r.current_recruit_state().unwrap().shop_frozen, Some(true));

    send_option(&mut r, 2, 0, 0);
    feed(
        &mut r,
        "BLOCK_START BlockType=PLAY Entity=[entityName=解冻 id=910 zone=PLAY zonePos=0 cardId=TB_BaconShopLockAll_Button player=4] EffectCardId= EffectIndex=0 Target=0 SubOption=-1",
    );
    feed(&mut r, "BLOCK_END");
    assert_eq!(r.is_shop_frozen(), Some(false));
}

#[test]
fn activate_keyword_is_static_and_live_availability_is_separate() {
    let mut r = base_runtime();
    create_entity(
        &mut r,
        920,
        "BG_PLAIN",
        &[
            ("CONTROLLER", "4"),
            ("CARDTYPE", "MINION"),
            ("ZONE", "PLAY"),
            ("ZONE_POSITION", "1"),
            // HAS_ACTIVATE_POWER is broadly present and must not make a plain
            // card live-activatable without a static Activate keyword.
            ("HAS_ACTIVATE_POWER", "1"),
        ],
    );
    create_entity(
        &mut r,
        921,
        "BG_ACTIVATE",
        &[
            ("CONTROLLER", "4"),
            ("CARDTYPE", "MINION"),
            ("ZONE", "PLAY"),
            ("ZONE_POSITION", "2"),
            ("HAS_ACTIVATE_POWER", "1"),
            ("INTERACTABLE_OBJECT", "1"),
        ],
    );

    let board = r.current_board();
    let plain = board.iter().find(|card| card.card_id.as_deref() == Some("BG_PLAIN")).unwrap();
    let active = board.iter().find(|card| card.card_id.as_deref() == Some("BG_ACTIVATE")).unwrap();

    assert!(!plain.keywords.activate_keyword);
    assert!(!plain.keywords.activate_available_now);
    assert!(active.keywords.activate_keyword);
    assert!(active.keywords.activate_available_now);
}

#[test]
fn numeric_show_entity_reconstructs_full_refresh_and_end_uses_last_stable_shop() {
    let mut r = base_runtime();
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES value=10");
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES_USED value=0");
    create_entity(
        &mut r,
        940,
        "BG_TTN_401",
        &[("CONTROLLER", "12"), ("CARDTYPE", "MINION"), ("ZONE", "PLAY"), ("ZONE_POSITION", "1")],
    );
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=1");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=STEP value=MAIN_ACTION");
    r.flush_pending_shop_revision();

    send_option(&mut r, 5, 0, 0);
    feed(&mut r, "BLOCK_START BlockType=PLAY Entity=[entityName=刷新 id=81 zone=PLAY zonePos=0 cardId=TB_BaconShop_8p_Reroll_Button player=4] EffectCardId= EffectIndex=0 Target=0 SubOption=-1");
    feed(&mut r, "TAG_CHANGE Entity=940 tag=ZONE value=REMOVEDFROMGAME");

    for (id, pos) in [(950, 1), (951, 2), (952, 3), (953, 4), (954, 5)] {
        feed(&mut r, &format!("FULL_ENTITY - Creating ID={id} CardID="));
        feed(&mut r, "    tag=CONTROLLER value=4");
        feed(&mut r, "    tag=ZONE value=SETASIDE");
        feed(&mut r, &format!("TAG_CHANGE Entity={id} tag=CONTROLLER value=12"));
        feed(&mut r, &format!("SHOW_ENTITY - Updating Entity={id} CardID=BG_TTN_401"));
        feed(&mut r, "    tag=CONTROLLER value=12");
        feed(&mut r, "    tag=CARDTYPE value=MINION");
        feed(&mut r, "    tag=ZONE value=PLAY");
        feed(&mut r, &format!("    tag=ENTITY_ID value={id}"));
        feed(&mut r, &format!("TAG_CHANGE Entity={id} tag=ZONE_POSITION value={pos}"));
    }
    feed(&mut r, "BLOCK_END");
    let emitted = r.flush_pending_shop_revision();
    let cards = emitted
        .iter()
        .find_map(|event| match event {
            HarnessEvent::ShopUpdated { card_ids, .. } => Some(card_ids.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(cards.len(), 5);
    assert_eq!(r.current_shop_card_ids().len(), 5);

    // Bob's pre-combat cleanup is transient and must not replace the last
    // published full shop with a shrinking 4/3/2/1-card snapshot.
    for id in [950, 951, 952, 953, 954] {
        feed(&mut r, &format!("TAG_CHANGE Entity={id} tag=ZONE value=REMOVEDFROMGAME"));
    }
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=2");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=3");

    let recruit = r.archive().round(1).unwrap().recruit.as_ref().unwrap();
    assert_eq!(recruit.end.shop.len(), 5);
    assert_eq!(recruit.shop_revisions.last().unwrap().cards.len(), 5);
}

#[test]
fn send_option_anchor_survives_nested_blocks_and_ignores_system_power() {
    let mut r = base_runtime();
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES value=9");
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES_USED value=0");
    create_entity(
        &mut r,
        960,
        "TB_BaconShop_UpdateDmgCap",
        &[("CONTROLLER", "4"), ("CARDTYPE", "SPELL"), ("ZONE", "SETASIDE")],
    );
    create_entity(
        &mut r,
        961,
        "TB_BaconShop_8p_Reroll_Button",
        &[("CONTROLLER", "4"), ("CARDTYPE", "GAME_MODE_BUTTON"), ("ZONE", "PLAY")],
    );
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=1");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=STEP value=MAIN_ACTION");

    // A legitimate parent block remains open across the independent input.
    feed(&mut r, "BLOCK_START BlockType=TRIGGER Entity=[entityName=parent id=999 zone=PLAY zonePos=0 cardId= player=4] EffectCardId= EffectIndex=0 Target=0 SubOption=-1");
    send_option(&mut r, 12, 0, 0);

    // Internal local-controller POWER must not consume the user-input anchor.
    feed(&mut r, "BLOCK_START BlockType=POWER Entity=[entityName=Update Damage Cap id=960 zone=SETASIDE zonePos=0 cardId=TB_BaconShop_UpdateDmgCap player=4] EffectCardId= EffectIndex=0 Target=0 SubOption=-1");
    feed(&mut r, "BLOCK_END");

    // The later semantic user PLAY is recognized even while parent depth > 0.
    feed(&mut r, "BLOCK_START BlockType=PLAY Entity=[entityName=刷新 id=961 zone=PLAY zonePos=0 cardId=TB_BaconShop_8p_Reroll_Button player=4] EffectCardId= EffectIndex=0 Target=0 SubOption=-1");
    feed(&mut r, "BLOCK_END");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=2");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=3");

    let recruit = r.archive().round(1).unwrap().recruit.as_ref().unwrap();
    assert_eq!(recruit.actions.len(), 1);
    assert_eq!(recruit.actions[0].kind, RecruitActionKind::Refresh);
}

#[test]
fn result_screen_hero_clone_does_not_override_logical_final_place() {
    let mut r = base_runtime();
    feed(&mut r, "TAG_CHANGE Entity=100 tag=PLAYER_LEADERBOARD_PLACE value=3");

    create_entity(
        &mut r,
        970,
        "MY_HERO",
        &[
            ("CONTROLLER", "4"),
            ("CARDTYPE", "HERO"),
            ("ZONE", "SETASIDE"),
            // Raw result clones can receive place before PLAYER_ID. The
            // override tag is already present and must suppress both orders.
            ("BACON_PLAYER_RESULTS_HERO_OVERRIDE", "12345"),
            ("PLAYER_LEADERBOARD_PLACE", "1"),
            ("PLAYER_ID", "4"),
        ],
    );

    // Any later event refreshes archive metadata.
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=STEP value=FINAL_WRAPUP");
    assert_eq!(r.archive().final_place, Some(3));
    let local = r
        .archive()
        .lobby_players
        .iter()
        .find(|player| player.is_local)
        .unwrap();
    assert_eq!(local.hero.entity_id, 100);
    assert_eq!(local.hero.leaderboard_place, Some(3));
}

#[test]
fn activate_requires_static_keyword_and_interactable_live_state() {
    let mut r = base_runtime();
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES value=10");
    feed(&mut r, "TAG_CHANGE Entity=11 tag=RESOURCES_USED value=0");
    create_entity(
        &mut r,
        980,
        "BG_PLAIN",
        &[("CONTROLLER", "4"), ("CARDTYPE", "MINION"), ("ZONE", "PLAY"), ("ZONE_POSITION", "1"), ("HAS_ACTIVATE_POWER", "1")],
    );
    create_entity(
        &mut r,
        981,
        "BG_ACTIVATE",
        &[("CONTROLLER", "4"), ("CARDTYPE", "MINION"), ("ZONE", "PLAY"), ("ZONE_POSITION", "2"), ("HAS_ACTIVATE_POWER", "1"), ("INTERACTABLE_OBJECT", "1")],
    );
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=1");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=STEP value=MAIN_ACTION");

    // Plain minion POWER must not be classified as Activate.
    send_option(&mut r, 1, 980, 0);
    feed(&mut r, "BLOCK_START BlockType=PLAY Entity=[entityName=普通随从 id=980 zone=PLAY zonePos=1 cardId=BG_PLAIN player=4] EffectCardId= EffectIndex=0 Target=0 SubOption=-1");
    feed(&mut r, "BLOCK_END");

    // A later real Activate can still consume that pending input only if its
    // own SendOption is emitted.
    send_option(&mut r, 2, 981, 0);
    feed(&mut r, "BLOCK_START BlockType=PLAY Entity=[entityName=测试激活随从 id=981 zone=PLAY zonePos=2 cardId=BG_ACTIVATE player=4] EffectCardId= EffectIndex=0 Target=0 SubOption=-1");
    feed(&mut r, "BLOCK_END");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=2");
    feed(&mut r, "TAG_CHANGE Entity=GameEntity tag=TURN value=3");

    let recruit = r.archive().round(1).unwrap().recruit.as_ref().unwrap();
    assert_eq!(recruit.actions.len(), 1);
    assert_eq!(recruit.actions[0].kind, RecruitActionKind::Activate);
}


#[test]
fn choice_updates_reclassify_trinket_after_source_arrives() {
    let mut r = base_runtime();
    let opened = r.feed_line(
        "D 10:00:00.0000000 GameState.DebugPrintEntityChoices() - id=77 Player=Me TaskList=1 ChoiceType=GENERAL CountMin=1 CountMax=1",
    );
    assert!(opened
        .iter()
        .any(|event| matches!(event, HarnessEvent::ChoiceOpened { id: 77 })));
    assert_eq!(r.current_choice().unwrap().kind, ChoiceKind::Discover);

    let updated = r.feed_line(
        "D 10:00:00.0000000 GameState.DebugPrintEntityChoices() - Source=[entityName=饰品选择 id=7000 zone=PLAY zonePos=0 cardId=BG30_Trinket_1st player=4]",
    );
    assert!(updated
        .iter()
        .any(|event| matches!(event, HarnessEvent::ChoiceUpdated { id: 77 })));
    assert_eq!(r.current_choice().unwrap().kind, ChoiceKind::Trinket);
}

#[test]
fn choice_updates_can_reclassify_trinket_from_option_card_id() {
    let mut r = base_runtime();
    r.feed_line(
        "D 10:00:00.0000000 GameState.DebugPrintEntityChoices() - id=78 Player=Me TaskList=1 ChoiceType=GENERAL CountMin=1 CountMax=1",
    );
    assert_eq!(r.current_choice().unwrap().kind, ChoiceKind::Discover);

    let updated = r.feed_line(
        "D 10:00:00.0000000 GameState.DebugPrintEntityChoices() -   Entities[0]=[entityName=测试饰品 id=7100 zone=SETASIDE zonePos=0 cardId=BG_TRINKET_TEST player=4]",
    );
    assert!(updated
        .iter()
        .any(|event| matches!(event, HarnessEvent::ChoiceUpdated { id: 78 })));
    assert_eq!(r.current_choice().unwrap().kind, ChoiceKind::Trinket);
}
