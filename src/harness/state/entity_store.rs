use std::collections::HashMap;

use crate::harness::power::{EntityDescriptor, EntityIdentity, PowerEvent, TagTarget, TagValue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRecord {
    pub entity_id: u32,
    pub player_id: i32,
    pub has_real_account: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    pub id: u32,
    pub name: Option<String>,
    pub card_id: Option<String>,
    pub tags: HashMap<String, TagValue>,
}

impl Entity {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: None,
            card_id: None,
            tags: HashMap::new(),
        }
    }

    pub fn tag(&self, name: &str) -> Option<&TagValue> {
        self.tags.get(name)
    }

    pub fn tag_i64(&self, name: &str) -> Option<i64> {
        self.tag(name).and_then(TagValue::as_i64)
    }

    pub fn tag_symbol(&self, name: &str) -> Option<&str> {
        self.tag(name).and_then(TagValue::as_str)
    }

    fn merge_identity(&mut self, identity: &EntityIdentity) {
        if let Some(name) = &identity.name {
            self.name = Some(name.clone());
        }
        if let Some(card_id) = &identity.card_id {
            self.card_id = Some(card_id.clone());
        }
    }

    fn apply_descriptor(&mut self, descriptor: &EntityDescriptor) {
        self.merge_identity(&descriptor.identity);
        if let Some(zone) = &descriptor.zone {
            self.tags.insert("ZONE".to_owned(), TagValue::Symbol(zone.clone()));
        }
        if let Some(position) = descriptor.zone_position {
            self.tags.insert("ZONE_POSITION".to_owned(), TagValue::Int(position as i64));
        }
        if let Some(controller) = descriptor.controller {
            self.tags.insert("CONTROLLER".to_owned(), TagValue::Int(controller as i64));
        }
    }
}

/// Complete bottom-layer fact store. No strategy logic lives here.
#[derive(Debug, Default)]
pub struct EntityStore {
    entities: HashMap<u32, Entity>,
    game_entity_id: Option<u32>,
    game_entity_tags: HashMap<String, TagValue>,
    players: HashMap<i32, PlayerRecord>,
    player_names: HashMap<i32, String>,
    named_target_tags: HashMap<String, HashMap<String, TagValue>>,
    /// Logical leaderboard state keyed by Battlegrounds PLAYER_ID. Hero
    /// entities can transform or be replaced by results-screen clones, so
    /// placement must not be derived from whichever HERO entity currently
    /// wins a zone/id preference heuristic.
    leaderboard_place_by_player: HashMap<i32, i32>,
}

impl EntityStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entity(&self, id: u32) -> Option<&Entity> {
        self.entities.get(&id)
    }

    pub fn entities(&self) -> impl Iterator<Item = &Entity> {
        self.entities.values()
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    pub fn game_entity_id(&self) -> Option<u32> {
        self.game_entity_id
    }

    pub fn game_entity_tag(&self, tag: &str) -> Option<&TagValue> {
        self.game_entity_tags.get(tag)
    }

    pub fn player(&self, player_id: i32) -> Option<&PlayerRecord> {
        self.players.get(&player_id)
    }

    pub fn players(&self) -> impl Iterator<Item = &PlayerRecord> {
        self.players.values()
    }

    pub fn player_name(&self, player_id: i32) -> Option<&str> {
        self.player_names.get(&player_id).map(String::as_str)
    }

    pub fn local_player_id(&self) -> Option<i32> {
        self.players.values().find(|p| p.has_real_account).map(|p| p.player_id)
    }

    pub fn dummy_player_id(&self) -> Option<i32> {
        self.players.values().find(|p| !p.has_real_account).map(|p| p.player_id)
    }

    pub fn local_player_name(&self) -> Option<&str> {
        self.local_player_id().and_then(|id| self.player_name(id))
    }

    pub fn leaderboard_place(&self, player_id: i32) -> Option<i32> {
        self.leaderboard_place_by_player.get(&player_id).copied()
    }

    pub fn player_entity(&self, player_id: i32) -> Option<&Entity> {
        let entity_id = self.player(player_id)?.entity_id;
        self.entity(entity_id)
    }

    /// Gets the latest player tag. During a live match Hearthstone often
    /// emits later updates against the player's *name* rather than the numeric
    /// Player entity, so name-targeted values must override the initial entity
    /// tags.
    pub fn player_tag(&self, player_id: i32, tag: &str) -> Option<&TagValue> {
        if let Some(name) = self.player_name(player_id) {
            if let Some(value) = self.named_target_tags.get(name).and_then(|tags| tags.get(tag)) {
                return Some(value);
            }
        }
        self.player_entity(player_id).and_then(|e| e.tag(tag))
    }

    pub fn apply(&mut self, event: &PowerEvent) {
        match event {
            PowerEvent::CreateGame => self.clear(),

            PowerEvent::GameEntityDeclared { entity_id } => {
                self.game_entity_id = Some(*entity_id);
            }

            PowerEvent::PlayerDeclared {
                entity_id,
                player_id,
                has_real_account,
            } => {
                self.players.insert(
                    *player_id,
                    PlayerRecord {
                        entity_id: *entity_id,
                        player_id: *player_id,
                        has_real_account: *has_real_account,
                    },
                );
                let entity = self.entities.entry(*entity_id).or_insert_with(|| Entity::new(*entity_id));
                entity.tags.insert("PLAYER_ID".to_owned(), TagValue::Int(*player_id as i64));
                entity.tags.insert("CARDTYPE".to_owned(), TagValue::Symbol("PLAYER".to_owned()));
            }

            PowerEvent::PlayerNameDeclared { player_id, name } => {
                self.player_names.insert(*player_id, name.clone());
            }

            PowerEvent::EntityCreated { id, card_id } => {
                // Entity ids are recycled: Creating always replaces the old entity.
                let mut entity = Entity::new(*id);
                entity.card_id = card_id.clone();
                self.entities.insert(*id, entity);
            }

            PowerEvent::EntityDefined(descriptor) => {
                let entity = self
                    .entities
                    .entry(descriptor.identity.id)
                    .or_insert_with(|| Entity::new(descriptor.identity.id));
                entity.apply_descriptor(descriptor);
            }

            PowerEvent::EntityCardChanged { identity, new_card_id } => {
                let entity = self
                    .entities
                    .entry(identity.id)
                    .or_insert_with(|| Entity::new(identity.id));
                entity.merge_identity(identity);
                entity.card_id = Some(new_card_id.clone());
            }

            PowerEvent::TagChanged { target, tag, value } => match target {
                TagTarget::Entity(identity) => {
                    let placement_update = {
                        let entity = self
                            .entities
                            .entry(identity.id)
                            .or_insert_with(|| Entity::new(identity.id));
                        entity.merge_identity(identity);
                        entity.tags.insert(tag.clone(), value.clone());

                        // Maintain placement as logical player state. Result-screen
                        // hero clones deliberately carry a temporary place=1 and
                        // must never overwrite the actual player's leaderboard.
                        let is_results_clone = entity
                            .tag_i64("BACON_PLAYER_RESULTS_HERO_OVERRIDE")
                            .unwrap_or(0)
                            != 0;
                        if is_results_clone {
                            None
                        } else {
                            match (
                                entity.tag_i64("PLAYER_ID").and_then(|v| i32::try_from(v).ok()),
                                entity
                                    .tag_i64("PLAYER_LEADERBOARD_PLACE")
                                    .and_then(|v| i32::try_from(v).ok()),
                            ) {
                                (Some(player_id), Some(place)) if player_id > 0 && place > 0 => {
                                    Some((player_id, place))
                                }
                                _ => None,
                            }
                        }
                    };
                    if let Some((player_id, place)) = placement_update {
                        self.leaderboard_place_by_player.insert(player_id, place);
                    }
                }
                TagTarget::GameEntity => {
                    self.game_entity_tags.insert(tag.clone(), value.clone());
                }
                TagTarget::PlayerName(name) => {
                    self.named_target_tags
                        .entry(name.clone())
                        .or_default()
                        .insert(tag.clone(), value.clone());
                }
            },

            PowerEvent::UserOptionSent { .. }
            | PowerEvent::BlockStarted { .. }
            | PowerEvent::BlockEnded => {}
        }
    }

    pub fn clear(&mut self) {
        self.entities.clear();
        self.game_entity_id = None;
        self.game_entity_tags.clear();
        self.players.clear();
        self.player_names.clear();
        self.named_target_tags.clear();
        self.leaderboard_place_by_player.clear();
    }
}
