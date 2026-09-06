use hearthcoach_harness::{
    demo::hdt_knowledge::HdtKnowledgeBase,
    harness::{CardCatalog, CardMeta},
};

fn pool_card(id: &str, name: &str, race: Option<&str>, tier: u8, in_pool: bool) -> CardMeta {
    CardMeta {
        card_id: id.to_owned(),
        name_zh_cn: Some(name.to_owned()),
        text_zh_cn: Some("测试文本".to_owned()),
        card_type: Some("MINION".to_owned()),
        race: race.map(ToOwned::to_owned),
        tavern_tier: Some(tier),
        in_bacon_pool: in_pool,
        ..Default::default()
    }
}

#[test]
fn hdt_knowledge_only_exposes_current_pool_and_available_tribes() {
    let catalog = CardCatalog::from_cards([
        pool_card("BG_MECH", "机械", Some("MECHANICAL"), 2, true),
        pool_card("BG_BEAST", "野兽", Some("BEAST"), 2, true),
        pool_card("BG_NEUTRAL", "中立", None, 3, true),
        pool_card("BG_OLD", "退池旧卡", Some("MECHANICAL"), 4, false),
    ]);
    let knowledge = HdtKnowledgeBase::from_catalog(catalog);
    let cards = knowledge.current_pool(&["MECH".to_owned()]);
    let ids = cards.into_iter().map(|card| card.card_id).collect::<Vec<_>>();
    assert_eq!(ids, vec!["BG_MECH", "BG_NEUTRAL"]);
    assert!(knowledge.validate_pool_card_id("BG_MECH", &["MECH".to_owned()]).is_some());
    assert!(knowledge.validate_pool_card_id("BG_BEAST", &["MECH".to_owned()]).is_none());
    assert!(knowledge.validate_pool_card_id("BG_OLD", &["MECH".to_owned()]).is_none());
}

#[test]
fn hdt_knowledge_prompt_is_not_arbitrarily_truncated() {
    let cards = (0..600)
        .map(|idx| pool_card(&format!("BG_POOL_{idx:03}"), &format!("卡{idx}"), Some("MECHANICAL"), (idx % 6 + 1) as u8, true))
        .collect::<Vec<_>>();
    let knowledge = HdtKnowledgeBase::from_catalog(CardCatalog::from_cards(cards));
    let prompt = knowledge.prompt_context(&["MECH".to_owned()]);
    assert!(prompt.contains("BG_POOL_000"));
    assert!(prompt.contains("BG_POOL_599"));
    assert_eq!(knowledge.current_pool_count(&["MECH".to_owned()]), 600);
}
