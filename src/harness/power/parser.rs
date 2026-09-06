use super::{EntityDescriptor, EntityId, EntityIdentity, PowerEvent, TagTarget, TagValue};

const GAME_STATE_MARKER: &str = "GameState.DebugPrintPower() - ";
const DEBUG_GAME_MARKER: &str = "GameState.DebugPrintGame() - ";
const SEND_OPTION_MARKER: &str = "GameState.SendOption() - ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineParseResult {
    IgnoredNonAuthoritative,
    IgnoredAuthoritative,
    Event(PowerEvent),
}

#[derive(Debug, Clone)]
enum PendingTarget {
    Entity(EntityIdentity),
    GameEntity,
}

#[derive(Debug, Default)]
pub struct PowerParser {
    pending_target: Option<PendingTarget>,
}

impl PowerParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.pending_target = None;
    }

    pub fn parse_line(&mut self, line: &str) -> LineParseResult {
        // SendOption is the authoritative client-side user input boundary. It
        // is intentionally parsed even though it is not an entity mutation.
        if let Some((_, payload)) = line.split_once(SEND_OPTION_MARKER) {
            if let Some(event) = self.parse_send_option(payload.trim()) {
                self.pending_target = None;
                return LineParseResult::Event(event);
            }
            return LineParseResult::IgnoredNonAuthoritative;
        }

        // Player names are printed by DebugPrintGame rather than DebugPrintPower.
        if let Some((_, payload)) = line.split_once(DEBUG_GAME_MARKER) {
            if let Some(event) = self.parse_player_name(payload.trim()) {
                return LineParseResult::Event(event);
            }
            return LineParseResult::IgnoredNonAuthoritative;
        }

        let Some((_, payload)) = line.split_once(GAME_STATE_MARKER) else {
            return LineParseResult::IgnoredNonAuthoritative;
        };

        let trimmed = payload.trim();

        if trimmed.starts_with("tag=") {
            return self.parse_pending_tag(trimmed);
        }

        self.pending_target = None;

        if trimmed == "CREATE_GAME" || trimmed.starts_with("CREATE_GAME ") {
            return LineParseResult::Event(PowerEvent::CreateGame);
        }

        if let Some(rest) = trimmed.strip_prefix("GameEntity EntityID=") {
            let Some(entity_id) = leading_u32(rest) else {
                return LineParseResult::IgnoredAuthoritative;
            };
            self.pending_target = Some(PendingTarget::GameEntity);
            return LineParseResult::Event(PowerEvent::GameEntityDeclared { entity_id });
        }

        if trimmed.starts_with("Player EntityID=") {
            return self.parse_player(trimmed);
        }

        if let Some(rest) = trimmed.strip_prefix("FULL_ENTITY - Creating ") {
            return self.parse_entity_creating(rest);
        }

        if let Some(rest) = trimmed.strip_prefix("FULL_ENTITY - Updating ") {
            return self.parse_entity_defined(rest);
        }

        if let Some(rest) = trimmed.strip_prefix("SHOW_ENTITY - Updating ") {
            return self.parse_entity_defined(rest);
        }

        if let Some(rest) = trimmed.strip_prefix("HIDE_ENTITY - ") {
            return self.parse_hide_entity(rest);
        }

        if let Some(rest) = trimmed.strip_prefix("CHANGE_ENTITY - Updating ") {
            return self.parse_change_entity(rest);
        }

        if let Some(rest) = trimmed.strip_prefix("TAG_CHANGE ") {
            return self.parse_tag_change(rest);
        }

        if let Some(rest) = trimmed.strip_prefix("BLOCK_START ") {
            return self.parse_block_start(rest);
        }

        if trimmed == "BLOCK_END" || trimmed.starts_with("BLOCK_END ") {
            return LineParseResult::Event(PowerEvent::BlockEnded);
        }

        LineParseResult::IgnoredAuthoritative
    }

    fn parse_send_option(&self, payload: &str) -> Option<PowerEvent> {
        let selected_option = signed_value_after(payload, "selectedOption=")?;
        let selected_target = signed_value_after(payload, "selectedTarget=")
            .filter(|id| *id > 0)
            .and_then(|id| u32::try_from(id).ok());
        let selected_position = signed_value_after(payload, "selectedPosition=");
        Some(PowerEvent::UserOptionSent {
            selected_option,
            selected_target,
            selected_position,
        })
    }

    fn parse_player_name(&self, payload: &str) -> Option<PowerEvent> {
        let player_id = signed_value_after(payload, "PlayerID=")?;
        let name_pos = payload.find("PlayerName=")? + "PlayerName=".len();
        let name = payload[name_pos..].trim();
        if name.is_empty() {
            return None;
        }
        Some(PowerEvent::PlayerNameDeclared {
            player_id,
            name: name.to_owned(),
        })
    }

    fn parse_pending_tag(&mut self, line: &str) -> LineParseResult {
        let Some(target) = self.pending_target.clone() else {
            return LineParseResult::IgnoredAuthoritative;
        };
        let Some((tag, value)) = split_tag_value(line) else {
            return LineParseResult::IgnoredAuthoritative;
        };

        let target = match target {
            PendingTarget::Entity(identity) => TagTarget::Entity(identity),
            PendingTarget::GameEntity => TagTarget::GameEntity,
        };

        LineParseResult::Event(PowerEvent::TagChanged {
            target,
            tag: tag.to_owned(),
            value: TagValue::parse(value),
        })
    }

    fn parse_player(&mut self, line: &str) -> LineParseResult {
        let Some(entity_id) = unsigned_value_after(line, "EntityID=") else {
            return LineParseResult::IgnoredAuthoritative;
        };
        let Some(player_id) = signed_value_after(line, "PlayerID=") else {
            return LineParseResult::IgnoredAuthoritative;
        };

        let hi = token_after(line, "hi=").unwrap_or("0");
        let lo = token_after(line, "lo=").unwrap_or("0");
        let has_real_account = !numeric_token_is_zero(hi) || !numeric_token_is_zero(lo);

        self.pending_target = Some(PendingTarget::Entity(EntityIdentity {
            id: entity_id,
            name: None,
            card_id: None,
        }));

        LineParseResult::Event(PowerEvent::PlayerDeclared {
            entity_id,
            player_id,
            has_real_account,
        })
    }

    fn parse_entity_creating(&mut self, line: &str) -> LineParseResult {
        let Some(id) = unsigned_value_after(line, "ID=") else {
            return LineParseResult::IgnoredAuthoritative;
        };

        let card_id = value_until_whitespace(line, "CardID=")
            .filter(|s| !s.is_empty())
            .map(str::to_owned);

        self.pending_target = Some(PendingTarget::Entity(EntityIdentity {
            id,
            name: None,
            card_id: card_id.clone(),
        }));

        LineParseResult::Event(PowerEvent::EntityCreated { id, card_id })
    }

    fn parse_entity_defined(&mut self, line: &str) -> LineParseResult {
        // Hearthstone emits two shapes here:
        //   SHOW_ENTITY - Updating [entityName=... id=123 ...]
        //   SHOW_ENTITY - Updating Entity=123 CardID=...
        // The latter is heavily used when a freshly rolled shop card moves
        // from a hidden SETASIDE candidate to Bob's visible PLAY zone.
        let mut descriptor = if let Some(descriptor) = Self::parse_descriptor(line) {
            descriptor
        } else if let Some(identity) = identity_after_named_key(line, "Entity=") {
            EntityDescriptor {
                identity,
                zone: None,
                zone_position: None,
                controller: None,
            }
        } else if let Some(id) = unsigned_value_after(line, "EntityID=") {
            EntityDescriptor {
                identity: EntityIdentity {
                    id,
                    name: None,
                    card_id: None,
                },
                zone: None,
                zone_position: None,
                controller: None,
            }
        } else {
            return LineParseResult::IgnoredAuthoritative;
        };

        if let Some(card_id) = value_until_whitespace(line, "CardID=") {
            if !card_id.is_empty() {
                descriptor.identity.card_id = Some(card_id.to_owned());
            }
        }

        self.pending_target = Some(PendingTarget::Entity(descriptor.identity.clone()));
        LineParseResult::Event(PowerEvent::EntityDefined(descriptor))
    }

    fn parse_hide_entity(&mut self, line: &str) -> LineParseResult {
        let Some(tag_pos) = line.find(" tag=") else {
            return LineParseResult::IgnoredAuthoritative;
        };
        let subject = line[..tag_pos].trim();
        let rest = &line[tag_pos + " tag=".len()..];
        let Some(value_pos) = rest.find(" value=") else {
            return LineParseResult::IgnoredAuthoritative;
        };
        let tag = rest[..value_pos].trim();
        let value = rest[value_pos + " value=".len()..].trim();
        let Some(raw_target) = subject.strip_prefix("Entity=") else {
            return LineParseResult::IgnoredAuthoritative;
        };
        let raw_target = raw_target.trim();
        let target = if raw_target.starts_with('[') {
            let Some(descriptor) = Self::parse_descriptor(raw_target) else {
                return LineParseResult::IgnoredAuthoritative;
            };
            TagTarget::Entity(descriptor.identity)
        } else if let Some(id) = leading_u32(raw_target) {
            TagTarget::Entity(EntityIdentity {
                id,
                name: None,
                card_id: None,
            })
        } else {
            return LineParseResult::IgnoredAuthoritative;
        };
        self.pending_target = None;
        LineParseResult::Event(PowerEvent::TagChanged {
            target,
            tag: tag.to_owned(),
            value: TagValue::parse(value),
        })
    }

    fn parse_change_entity(&self, line: &str) -> LineParseResult {
        let identity = if let Some(descriptor) = Self::parse_descriptor(line) {
            descriptor.identity
        } else if let Some(id) = entity_id_after_entity_key(line) {
            EntityIdentity {
                id,
                name: None,
                card_id: None,
            }
        } else {
            return LineParseResult::IgnoredAuthoritative;
        };

        let Some(card_id) = value_until_whitespace(line, "CardID=") else {
            return LineParseResult::IgnoredAuthoritative;
        };
        if card_id.is_empty() {
            return LineParseResult::IgnoredAuthoritative;
        }

        LineParseResult::Event(PowerEvent::EntityCardChanged {
            identity,
            new_card_id: card_id.to_owned(),
        })
    }

    fn parse_tag_change(&self, line: &str) -> LineParseResult {
        let Some(tag_pos) = line.find(" tag=") else {
            return LineParseResult::IgnoredAuthoritative;
        };
        let subject = line[..tag_pos].trim();
        let rest = &line[tag_pos + " tag=".len()..];
        let Some(value_pos) = rest.find(" value=") else {
            return LineParseResult::IgnoredAuthoritative;
        };

        let tag = rest[..value_pos].trim();
        let value = rest[value_pos + " value=".len()..].trim();
        if tag.is_empty() {
            return LineParseResult::IgnoredAuthoritative;
        }

        let Some(raw_target) = subject.strip_prefix("Entity=") else {
            return LineParseResult::IgnoredAuthoritative;
        };
        let raw_target = raw_target.trim();

        let target = if raw_target == "GameEntity" {
            TagTarget::GameEntity
        } else if raw_target.starts_with('[') {
            let Some(descriptor) = Self::parse_descriptor(raw_target) else {
                return LineParseResult::IgnoredAuthoritative;
            };
            // Positional fields embedded in TAG_CHANGE are stale cached values.
            TagTarget::Entity(descriptor.identity)
        } else if let Ok(id) = raw_target.parse::<EntityId>() {
            TagTarget::Entity(EntityIdentity {
                id,
                name: None,
                card_id: None,
            })
        } else {
            TagTarget::PlayerName(raw_target.to_owned())
        };

        LineParseResult::Event(PowerEvent::TagChanged {
            target,
            tag: tag.to_owned(),
            value: TagValue::parse(value),
        })
    }

    fn parse_block_start(&self, line: &str) -> LineParseResult {
        let block_type = value_until_whitespace(line, "BlockType=")
            .unwrap_or("")
            .to_owned();
        if block_type.is_empty() {
            return LineParseResult::IgnoredAuthoritative;
        }

        let source = identity_after_named_key(line, "Entity=");
        let target = identity_after_named_key(line, "Target=");
        let effect_card_id = value_until_whitespace(line, "EffectCardId=")
            .filter(|s| !s.is_empty())
            .map(str::to_owned);

        LineParseResult::Event(PowerEvent::BlockStarted {
            block_type,
            source,
            target,
            effect_card_id,
        })
    }

    /// Parse one Hearthstone entity descriptor while allowing localized names
    /// and nested brackets inside the name. Only the first *balanced* outer
    /// descriptor is considered, so a following Target=[...] cannot leak into
    /// the Entity=[...] parse.
    pub fn parse_descriptor(input: &str) -> Option<EntityDescriptor> {
        let descriptor = first_balanced_descriptor(input)?;
        let body = descriptor.strip_prefix('[')?.strip_suffix(']')?;

        let player_key = body.rfind(" player=")?;
        let card_id_key = body[..player_key].rfind(" cardId=")?;
        let zone_pos_key = body[..card_id_key].rfind(" zonePos=")?;
        let zone_key = body[..zone_pos_key].rfind(" zone=")?;
        let id_key = body[..zone_key].rfind(" id=")?;

        let name_prefix = &body[..id_key];
        let name_part = name_prefix
            .strip_prefix("entityName=")
            .or_else(|| name_prefix.strip_prefix("name="))?;
        let id_str = body[id_key + " id=".len()..zone_key].trim();
        let zone = body[zone_key + " zone=".len()..zone_pos_key].trim();
        let zone_pos_str = body[zone_pos_key + " zonePos=".len()..card_id_key].trim();
        let card_id = body[card_id_key + " cardId=".len()..player_key].trim();
        let player_str = body[player_key + " player=".len()..].trim();

        let id = id_str.parse::<EntityId>().ok()?;
        let zone_position = zone_pos_str.parse::<i32>().ok();
        let controller = player_str.parse::<i32>().ok();

        Some(EntityDescriptor {
            identity: EntityIdentity {
                id,
                name: (!name_part.is_empty()).then(|| name_part.to_owned()),
                card_id: (!card_id.is_empty()).then(|| card_id.to_owned()),
            },
            zone: (!zone.is_empty()).then(|| zone.to_owned()),
            zone_position,
            controller,
        })
    }

}

fn split_tag_value(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("tag=")?;
    let value_pos = rest.find(" value=")?;
    Some((rest[..value_pos].trim(), rest[value_pos + " value=".len()..].trim()))
}

fn value_until_whitespace<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let start = line.find(key)? + key.len();
    let rest = &line[start..];
    Some(rest.split_whitespace().next().unwrap_or(""))
}

fn token_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    value_until_whitespace(line, key)
}

fn leading_u32(value: &str) -> Option<u32> {
    let digits: String = value.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn unsigned_value_after(line: &str, key: &str) -> Option<u32> {
    let value = value_until_whitespace(line, key)?;
    value
        .trim_matches(|c: char| !c.is_ascii_digit())
        .parse::<u32>()
        .ok()
}

fn signed_value_after(line: &str, key: &str) -> Option<i32> {
    let start = line.find(key)? + key.len();
    let rest = &line[start..];
    let token: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    token.parse::<i32>().ok()
}

fn numeric_token_is_zero(token: &str) -> bool {
    let digits: String = token.chars().filter(|c| c.is_ascii_digit()).collect();
    !digits.is_empty() && digits.chars().all(|c| c == '0')
}

fn entity_id_after_entity_key(line: &str) -> Option<EntityId> {
    identity_after_named_key(line, "Entity=").map(|identity| identity.id)
}

fn identity_after_named_key(line: &str, key: &str) -> Option<EntityIdentity> {
    let raw = value_after_named_entity(line, key)?;
    let raw = raw.trim();
    if raw.is_empty() || raw == "0" || raw.eq_ignore_ascii_case("GameEntity") {
        return None;
    }
    if raw.starts_with('[') {
        return PowerParser::parse_descriptor(raw).map(|d| d.identity);
    }
    leading_u32(raw).map(|id| EntityIdentity {
        id,
        name: None,
        card_id: None,
    })
}

/// Return exactly one value after a named Entity/Target key. If the value is a
/// bracketed descriptor, scan bracket depth so nested `[cardType=INVALID]`
/// inside localized names is handled correctly.
fn value_after_named_entity<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let start = line.find(key)? + key.len();
    let rest = line[start..].trim_start();
    if rest.starts_with('[') {
        return first_balanced_descriptor(rest);
    }
    Some(rest.split_whitespace().next().unwrap_or(""))
}

fn first_balanced_descriptor(input: &str) -> Option<&str> {
    let open = input.find('[')?;
    let mut depth = 0usize;
    for (offset, ch) in input[open..].char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    let end = open + offset + ch.len_utf8();
                    return Some(&input[open..end]);
                }
            }
            _ => {}
        }
    }
    None
}
