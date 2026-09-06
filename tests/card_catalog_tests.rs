use hearthcoach_harness::harness::{normalize_tribe, CardCatalog, CardMeta, CardSnapshot, Entity, TagValue};

fn normal_mech() -> CardMeta {
    CardMeta {
        card_id: "BG_TEST".to_owned(),
        name_zh_cn: Some("测试机械".to_owned()),
        name_en_us: Some("Test Mech".to_owned()),
        card_type: Some("MINION".to_owned()),
        race: Some("MECHANICAL".to_owned()),
        tavern_tier: Some(3),
        attack: Some(4),
        health: Some(5),
        ..Default::default()
    }
}

#[test]
fn static_catalog_overrides_unknown_entity_name() {
    let catalog = CardCatalog::from_cards([normal_mech()]);
    let mut entity = Entity::new(1);
    entity.card_id = Some("BG_TEST".to_owned());
    entity.name = Some("UNKNOWN ENTITY [cardType=INVALID]".to_owned());
    entity
        .tags
        .insert("CARDTYPE".to_owned(), TagValue::Symbol("MINION".to_owned()));

    let snapshot = CardSnapshot::from_entity_with_catalog(&entity, Some(&catalog));
    assert_eq!(snapshot.name.as_deref(), Some("测试机械"));
    assert_eq!(snapshot.tribes, vec!["MECH"]);
}

#[test]
fn golden_card_falls_back_to_normal_metadata() {
    let catalog = CardCatalog::from_cards([normal_mech()]);
    let resolved = catalog.resolve("BG_TEST_G").unwrap();
    assert_eq!(resolved.preferred_name(), Some("测试机械"));
    assert_eq!(resolved.tribes(), vec!["MECH"]);
    assert!(resolved.premium());
}

#[test]
fn catalog_exposes_text_and_mechanics() {
    let catalog = CardCatalog::from_cards([CardMeta {
        card_id: "BG_TEXT".to_owned(),
        name_zh_cn: Some("文本测试".to_owned()),
        text_zh_cn: Some("战吼：获得+1/+1。".to_owned()),
        mechanics: vec!["BATTLECRY".to_owned(), "TAUNT".to_owned()],
        card_type: Some("MINION".to_owned()),
        ..Default::default()
    }]);
    let card = catalog.resolve("BG_TEXT").unwrap();
    assert_eq!(card.preferred_text(), Some("战吼：获得+1/+1。"));
    assert!(card.mechanics().contains(&"BATTLECRY".to_owned()));
}


#[test]
fn catalog_keeps_static_activate_keyword_separate_from_live_state() {
    let catalog = CardCatalog::from_cards([CardMeta {
        card_id: "BG_ACT".to_owned(),
        name_zh_cn: Some("激活测试".to_owned()),
        card_type: Some("MINION".to_owned()),
        activate_keyword: true,
        ..Default::default()
    }]);
    let resolved = catalog.resolve("BG_ACT").unwrap();
    assert!(resolved.activate_keyword());
}


#[test]
fn numeric_hearthmirror_race_values_are_normalized() {
    assert_eq!(normalize_tribe("17").as_deref(), Some("MECH"));
    assert_eq!(normalize_tribe("18").as_deref(), Some("ELEMENTALS"));
    assert_eq!(normalize_tribe("20").as_deref(), Some("BEAST"));
    assert_eq!(normalize_tribe("24").as_deref(), Some("DRAGON"));
    assert_eq!(normalize_tribe("43").as_deref(), Some("QUILLBOAR"));
}

#[test]
fn catalog_preserves_hdt_bacon_pool_membership() {
    let catalog = CardCatalog::from_cards([CardMeta {
        card_id: "BG_POOL".to_owned(),
        name_zh_cn: Some("当前池随从".to_owned()),
        card_type: Some("MINION".to_owned()),
        tavern_tier: Some(2),
        in_bacon_pool: true,
        ..Default::default()
    }]);
    let resolved = catalog.resolve("BG_POOL").unwrap();
    assert!(resolved.in_bacon_pool());
}
