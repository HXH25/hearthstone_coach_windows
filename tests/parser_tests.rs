use hearthcoach_harness::harness::{
    power::{LineParseResult, PowerEvent, PowerParser, TagTarget},
    EntityStore,
};

fn gs(payload: &str) -> String {
    format!("D 14:57:49.9029830 GameState.DebugPrintPower() - {payload}")
}

#[test]
fn player_continuation_tags_are_kept() {
    let mut parser = PowerParser::new();
    let mut store = EntityStore::new();

    let lines = [
        gs("Player EntityID=11 PlayerID=4 GameAccountId=[hi=123 lo=456]"),
        gs("    tag=NEXT_OPPONENT_PLAYER_ID value=3"),
        gs("    tag=PLAYER_TECH_LEVEL value=1"),
    ];

    for line in lines {
        if let LineParseResult::Event(event) = parser.parse_line(&line) {
            store.apply(&event);
        }
    }

    assert_eq!(store.local_player_id(), Some(4));
    assert_eq!(
        store
            .player_tag(4, "NEXT_OPPONENT_PLAYER_ID")
            .and_then(|v| v.as_i64()),
        Some(3)
    );
    assert_eq!(
        store
            .player_tag(4, "PLAYER_TECH_LEVEL")
            .and_then(|v| v.as_i64()),
        Some(1)
    );
}

#[test]
fn game_entity_continuation_tags_are_kept() {
    let mut parser = PowerParser::new();
    let mut store = EntityStore::new();
    for line in [
        gs("GameEntity EntityID=10"),
        gs("    tag=BACON_TRINKETS_ACTIVE value=1"),
    ] {
        if let LineParseResult::Event(event) = parser.parse_line(&line) {
            store.apply(&event);
        }
    }
    assert_eq!(store.game_entity_id(), Some(10));
    assert_eq!(
        store
            .game_entity_tag("BACON_TRINKETS_ACTIVE")
            .and_then(|v| v.as_i64()),
        Some(1)
    );
}

#[test]
fn stale_tag_descriptor_position_is_not_reapplied() {
    let mut parser = PowerParser::new();
    let line = gs("TAG_CHANGE Entity=[entityName=X id=561 zone=PLAY zonePos=1 cardId=BG28_300 player=9] tag=ZONE value=REMOVEDFROMGAME");
    let LineParseResult::Event(PowerEvent::TagChanged { target, tag, .. }) =
        parser.parse_line(&line)
    else {
        panic!("expected tag change");
    };
    assert_eq!(tag, "ZONE");
    match target {
        TagTarget::Entity(identity) => assert_eq!(identity.id, 561),
        _ => panic!("wrong target"),
    }
}

#[test]
fn power_task_list_is_ignored() {
    let mut parser = PowerParser::new();
    let line = "D 14:57:49.0 PowerTaskList.DebugPrintPower() - TAG_CHANGE Entity=1 tag=ATK value=99";
    assert!(matches!(
        parser.parse_line(line),
        LineParseResult::IgnoredNonAuthoritative
    ));
}

#[test]
fn descriptor_allows_brackets_and_chinese() {
    let d = PowerParser::parse_descriptor(
        "[entityName=UNKNOWN ENTITY [cardType=INVALID] 中文 id=660 zone=SETASIDE zonePos=0 cardId=BG_X player=2]",
    )
    .unwrap();
    assert_eq!(d.identity.id, 660);
    assert_eq!(
        d.identity.name.as_deref(),
        Some("UNKNOWN ENTITY [cardType=INVALID] 中文")
    );
}

#[test]
fn block_start_parses_source_and_target_independently() {
    let mut parser = PowerParser::new();
    let line = gs(
        "BLOCK_START BlockType=PLAY Entity=[entityName=购买 id=80 zone=PLAY zonePos=0 cardId=TB_BaconShop_DragBuy player=1] EffectCardId= EffectIndex=0 Target=[entityName=星元自动机 id=419 zone=PLAY zonePos=2 cardId=BG_TTN_401 player=12] SubOption=-1",
    );
    let LineParseResult::Event(PowerEvent::BlockStarted {
        source,
        target,
        ..
    }) = parser.parse_line(&line)
    else {
        panic!("expected block start");
    };

    let source = source.unwrap();
    let target = target.unwrap();
    assert_eq!(source.id, 80);
    assert_eq!(source.card_id.as_deref(), Some("TB_BaconShop_DragBuy"));
    assert_eq!(target.id, 419);
    assert_eq!(target.card_id.as_deref(), Some("BG_TTN_401"));
}

#[test]
fn numeric_show_entity_updates_hidden_candidate_into_visible_shop_entity() {
    let mut parser = PowerParser::new();
    let mut store = EntityStore::new();
    let lines = [
        gs("FULL_ENTITY - Creating ID=2983 CardID="),
        gs("    tag=CONTROLLER value=7"),
        gs("    tag=ZONE value=SETASIDE"),
        gs("TAG_CHANGE Entity=2983 tag=CONTROLLER value=15"),
        gs("SHOW_ENTITY - Updating Entity=2983 CardID=BG36_508"),
        gs("    tag=CONTROLLER value=15"),
        gs("    tag=CARDTYPE value=MINION"),
        gs("    tag=ZONE value=PLAY"),
        gs("    tag=ENTITY_ID value=2983"),
        gs("    tag=TECH_LEVEL value=3"),
        gs("TAG_CHANGE Entity=2983 tag=ZONE_POSITION value=5"),
    ];

    for line in lines {
        if let LineParseResult::Event(event) = parser.parse_line(&line) {
            store.apply(&event);
        }
    }

    let entity = store.entity(2983).unwrap();
    assert_eq!(entity.card_id.as_deref(), Some("BG36_508"));
    assert_eq!(entity.tag_symbol("CARDTYPE"), Some("MINION"));
    assert_eq!(entity.tag_symbol("ZONE"), Some("PLAY"));
    assert_eq!(entity.tag_i64("CONTROLLER"), Some(15));
    assert_eq!(entity.tag_i64("ZONE_POSITION"), Some(5));
}

#[test]
fn numeric_hide_entity_is_applied_as_zone_change() {
    let mut parser = PowerParser::new();
    let mut store = EntityStore::new();
    for line in [
        gs("FULL_ENTITY - Creating ID=2983 CardID=BG36_508"),
        gs("    tag=ZONE value=PLAY"),
        gs("HIDE_ENTITY - Entity=2983 tag=ZONE value=SETASIDE"),
    ] {
        if let LineParseResult::Event(event) = parser.parse_line(&line) {
            store.apply(&event);
        }
    }
    assert_eq!(store.entity(2983).unwrap().tag_symbol("ZONE"), Some("SETASIDE"));
}

#[test]
fn send_option_is_parsed_as_user_input_boundary() {
    let mut parser = PowerParser::new();
    let line = "D 10:42:57.4429682 GameState.SendOption() - selectedOption=5 selectedSubOption=-1 selectedTarget=2983 selectedPosition=2";
    let LineParseResult::Event(PowerEvent::UserOptionSent {
        selected_option,
        selected_target,
        selected_position,
    }) = parser.parse_line(line)
    else {
        panic!("expected SendOption event");
    };
    assert_eq!(selected_option, 5);
    assert_eq!(selected_target, Some(2983));
    assert_eq!(selected_position, Some(2));
}
