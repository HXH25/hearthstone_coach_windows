use super::ChoiceEvent;
use crate::harness::power::PowerParser;

const CHOICES_MARKER: &str = "GameState.DebugPrintEntityChoices() - ";
const CHOSEN_MARKER: &str = "GameState.DebugPrintEntitiesChosen() - ";

#[derive(Debug, Default)]
pub struct ChoiceParser {
    current_choice_id: Option<u32>,
    current_chosen_id: Option<u32>,
}

impl ChoiceParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.current_choice_id = None;
        self.current_chosen_id = None;
    }

    pub fn parse_line(&mut self, line: &str) -> Option<ChoiceEvent> {
        if let Some((_, payload)) = line.split_once(CHOICES_MARKER) {
            return self.parse_choice_line(payload.trim());
        }
        if let Some((_, payload)) = line.split_once(CHOSEN_MARKER) {
            return self.parse_chosen_line(payload.trim());
        }
        None
    }

    fn parse_choice_line(&mut self, payload: &str) -> Option<ChoiceEvent> {
        if payload.starts_with("id=") {
            let id = unsigned_after(payload, "id=")?;
            let player_name = token_after(payload, "Player=")?.to_owned();
            let choice_type = token_after(payload, "ChoiceType=")?.to_owned();
            let count_min = unsigned_after(payload, "CountMin=")? as usize;
            let count_max = unsigned_after(payload, "CountMax=")? as usize;
            self.current_choice_id = Some(id);
            return Some(ChoiceEvent::Opened {
                id,
                player_name,
                choice_type,
                count_min,
                count_max,
            });
        }

        if let Some(raw) = payload.strip_prefix("Source=") {
            let id = self.current_choice_id?;
            let raw = raw.trim();
            return Some(ChoiceEvent::Source {
                id,
                source: PowerParser::parse_descriptor(raw),
                raw_source: raw.to_owned(),
            });
        }

        if payload.starts_with("Entities[") {
            let id = self.current_choice_id?;
            let index = index_inside_brackets(payload)?;
            let raw = payload.split_once('=')?.1.trim();
            let entity = PowerParser::parse_descriptor(raw)?;
            return Some(ChoiceEvent::Option { id, index, entity });
        }

        None
    }

    fn parse_chosen_line(&mut self, payload: &str) -> Option<ChoiceEvent> {
        if payload.starts_with("id=") {
            let id = unsigned_after(payload, "id=")?;
            let entities_count = unsigned_after(payload, "EntitiesCount=")? as usize;
            self.current_chosen_id = Some(id);
            return Some(ChoiceEvent::SelectionStarted { id, entities_count });
        }

        if payload.starts_with("Entities[") {
            let id = self.current_chosen_id?;
            let index = index_inside_brackets(payload)?;
            let raw = payload.split_once('=')?.1.trim();
            let entity = PowerParser::parse_descriptor(raw)?;
            return Some(ChoiceEvent::Selected { id, index, entity });
        }

        None
    }
}

fn token_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let start = line.find(key)? + key.len();
    Some(line[start..].split_whitespace().next().unwrap_or(""))
}

fn unsigned_after(line: &str, key: &str) -> Option<u32> {
    let token = token_after(line, key)?;
    token.trim_matches(|c: char| !c.is_ascii_digit()).parse().ok()
}

fn index_inside_brackets(line: &str) -> Option<usize> {
    let open = line.find('[')? + 1;
    let close = line[open..].find(']')? + open;
    line[open..close].parse().ok()
}
