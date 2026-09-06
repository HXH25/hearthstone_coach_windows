use hearthcoach_harness::harness::choice::{ChoiceEvent, ChoiceParser};

#[test]
fn parses_choice_and_selected_entity() {
    let mut parser = ChoiceParser::new();
    let header = "D 19:04:57.0 GameState.DebugPrintEntityChoices() - id=1 Player=Me TaskList=7 ChoiceType=MULLIGAN CountMin=1 CountMax=1";
    assert!(matches!(parser.parse_line(header), Some(ChoiceEvent::Opened { id: 1, .. })));

    let option = "D 19:04:57.0 GameState.DebugPrintEntityChoices() -   Entities[0]=[entityName=提克特斯 id=94 zone=HAND zonePos=1 cardId=TB_BaconShop_HERO_94 player=4]";
    assert!(matches!(parser.parse_line(option), Some(ChoiceEvent::Option { id: 1, index: 0, .. })));

    let chosen_header = "D 19:05:05.0 GameState.DebugPrintEntitiesChosen() - id=1 Player=Me EntitiesCount=1";
    assert!(matches!(parser.parse_line(chosen_header), Some(ChoiceEvent::SelectionStarted { id: 1, entities_count: 1 })));

    let chosen = "D 19:05:05.0 GameState.DebugPrintEntitiesChosen() -   Entities[0]=[entityName=提克特斯 id=94 zone=HAND zonePos=1 cardId=TB_BaconShop_HERO_94 player=4]";
    assert!(matches!(parser.parse_line(chosen), Some(ChoiceEvent::Selected { id: 1, index: 0, .. })));
}
