use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::harness::card_catalog::{normalize_tribe, CardCatalog};

use super::{Entity, EntityStore};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CardKeywords {
    pub taunt: bool,
    pub divine_shield: bool,
    pub reborn: bool,
    pub poisonous: bool,
    pub venomous: bool,
    pub windfury: bool,
    pub mega_windfury: bool,
    pub stealth: bool,
    pub battlecry: bool,
    pub deathrattle: bool,
    pub avenge: bool,
    pub rally: bool,
    pub end_of_turn: bool,
    pub start_of_combat: bool,
    pub choose_one: bool,
    pub modular: bool,
    pub magnetic: bool,
    /// Static CardDefs keyword: the card has an Activate mechanic.
    pub activate_keyword: bool,
    /// Live entity state: the Activate action is currently usable.
    pub activate_available_now: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardSnapshot {
    pub entity_id: u32,
    pub card_id: Option<String>,
    pub name: Option<String>,
    pub text: Option<String>,
    pub mechanics: Vec<String>,
    pub card_type: Option<String>,
    pub controller: Option<i32>,
    pub zone: Option<String>,
    pub zone_position: Option<i32>,
    pub attack: Option<i32>,
    pub health: Option<i32>,
    pub damage: Option<i32>,
    pub remaining_health: Option<i32>,
    pub tavern_tier: Option<u8>,
    /// Card/entity COST. For Battlegrounds minions this is normally 0 and is
    /// not the Bob purchase price. Tavern spells use it as their printed cost.
    #[serde(alias = "cost")]
    pub printed_cost: Option<i32>,
    pub is_golden: bool,
    pub tribes: Vec<String>,
    pub keywords: CardKeywords,
}

impl CardSnapshot {
    pub fn from_entity(entity: &Entity) -> Self {
        Self::from_entity_with_catalog(entity, None)
    }

    pub fn from_entity_with_catalog(entity: &Entity, catalog: Option<&CardCatalog>) -> Self {
        let resolved = entity
            .card_id
            .as_deref()
            .and_then(|card_id| catalog.and_then(|catalog| catalog.resolve(card_id)));

        let health = i32_tag(entity, "HEALTH").or_else(|| resolved.and_then(|c| c.health()));
        let damage = i32_tag(entity, "DAMAGE");
        let remaining_health = health.map(|h| h - damage.unwrap_or(0));

        let mut tribes = BTreeSet::new();
        for (key, value) in &entity.tags {
            if let Some(name) = key.strip_prefix("BACON_SUBSET_") {
                if value.as_i64().unwrap_or(0) != 0 {
                    if let Some(name) = normalize_tribe(name) {
                        tribes.insert(name);
                    }
                }
            }
        }
        if let Some(resolved) = resolved {
            tribes.extend(resolved.tribes());
        }

        let static_name = resolved.and_then(|c| c.preferred_name()).map(str::to_owned);
        let static_text = resolved.and_then(|c| c.preferred_text()).map(str::to_owned);
        let static_mechanics = resolved.map(|c| c.mechanics()).unwrap_or_default();
        let entity_name = trustworthy_entity_name(entity.name.as_deref()).map(str::to_owned);
        let dynamic_card_type = entity
            .tag_symbol("CARDTYPE")
            .filter(|value| !value.eq_ignore_ascii_case("INVALID"))
            .map(str::to_owned);

        Self {
            entity_id: entity.id,
            card_id: entity.card_id.clone(),
            name: static_name.or(entity_name),
            text: static_text,
            mechanics: static_mechanics.clone(),
            card_type: dynamic_card_type.or_else(|| resolved.and_then(|c| c.card_type()).map(str::to_owned)),
            controller: i32_tag(entity, "CONTROLLER"),
            zone: entity.tag_symbol("ZONE").map(str::to_owned),
            zone_position: i32_tag(entity, "ZONE_POSITION"),
            attack: i32_tag(entity, "ATK").or_else(|| resolved.and_then(|c| c.attack())),
            health,
            damage,
            remaining_health,
            tavern_tier: i32_tag(entity, "TECH_LEVEL")
                .and_then(|v| u8::try_from(v).ok())
                .filter(|v| *v > 0)
                .or_else(|| resolved.and_then(|c| c.tavern_tier())),
            printed_cost: i32_tag(entity, "COST").or_else(|| resolved.and_then(|c| c.cost())),
            is_golden: truthy(entity, "PREMIUM")
                || resolved.map(|c| c.premium()).unwrap_or(false)
                || entity.card_id.as_deref().map(|id| id.ends_with("_G")).unwrap_or(false),
            tribes: tribes.into_iter().collect(),
            keywords: CardKeywords {
                taunt: truthy(entity, "TAUNT") || has_mechanic(&static_mechanics, "Taunt"),
                divine_shield: truthy(entity, "DIVINE_SHIELD") || has_mechanic(&static_mechanics, "Divine Shield"),
                reborn: truthy(entity, "REBORN") || has_mechanic(&static_mechanics, "Reborn"),
                poisonous: truthy(entity, "POISONOUS") || has_mechanic(&static_mechanics, "Poisonous"),
                venomous: truthy(entity, "VENOMOUS") || has_mechanic(&static_mechanics, "Venomous"),
                windfury: truthy(entity, "WINDFURY") || has_mechanic(&static_mechanics, "Windfury"),
                mega_windfury: truthy(entity, "MEGA_WINDFURY") || has_mechanic(&static_mechanics, "Mega-Windfury"),
                stealth: truthy(entity, "STEALTH") || has_mechanic(&static_mechanics, "Stealth"),
                battlecry: truthy(entity, "BATTLECRY") || has_mechanic(&static_mechanics, "Battlecry"),
                deathrattle: truthy(entity, "DEATHRATTLE") || has_mechanic(&static_mechanics, "Deathrattle"),
                avenge: truthy(entity, "AVENGE") || has_mechanic(&static_mechanics, "Avenge"),
                rally: truthy(entity, "BACON_RALLY") || has_mechanic(&static_mechanics, "Rally"),
                end_of_turn: truthy(entity, "END_OF_TURN_TRIGGER"),
                start_of_combat: truthy(entity, "START_OF_COMBAT"),
                choose_one: truthy(entity, "CHOOSE_ONE") || has_mechanic(&static_mechanics, "Choose One"),
                modular: truthy(entity, "MODULAR") || has_mechanic(&static_mechanics, "Magnetic"),
                magnetic: truthy(entity, "MODULAR") || has_mechanic(&static_mechanics, "Magnetic"),
                activate_keyword: resolved.map(|c| c.activate_keyword()).unwrap_or(false),
                // HAS_ACTIVATE_POWER=1 is broadly stamped on Battlegrounds
                // heroes, buttons and ordinary minions. The actual live BG
                // Activate interaction is represented by INTERACTABLE_OBJECT.
                activate_available_now: resolved.map(|c| c.activate_keyword()).unwrap_or(false)
                    && truthy(entity, "INTERACTABLE_OBJECT")
                    && !truthy(entity, "CANT_PLAY")
                    && !truthy(entity, "EXHAUSTED"),
            },
        }
    }

    pub fn from_identity(
        entity_id: u32,
        card_id: Option<String>,
        name: Option<String>,
        controller: Option<i32>,
        zone: Option<String>,
        zone_position: Option<i32>,
        catalog: Option<&CardCatalog>,
    ) -> Self {
        let resolved = card_id
            .as_deref()
            .and_then(|card_id| catalog.and_then(|catalog| catalog.resolve(card_id)));
        let static_name = resolved.and_then(|c| c.preferred_name()).map(str::to_owned);
        let entity_name = trustworthy_entity_name(name.as_deref()).map(str::to_owned);
        let tribes = resolved.map(|c| c.tribes()).unwrap_or_default();
        let health = resolved.and_then(|c| c.health());

        Self {
            entity_id,
            card_id,
            name: static_name.or(entity_name),
            text: resolved.and_then(|c| c.preferred_text()).map(str::to_owned),
            mechanics: resolved.map(|c| c.mechanics()).unwrap_or_default(),
            card_type: resolved.and_then(|c| c.card_type()).map(str::to_owned),
            controller,
            zone,
            zone_position,
            attack: resolved.and_then(|c| c.attack()),
            health,
            damage: None,
            remaining_health: health,
            tavern_tier: resolved.and_then(|c| c.tavern_tier()),
            printed_cost: resolved.and_then(|c| c.cost()),
            is_golden: resolved.map(|c| c.premium()).unwrap_or(false),
            tribes,
            keywords: CardKeywords::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeroSnapshot {
    pub entity_id: u32,
    pub player_id: Option<i32>,
    pub card_id: Option<String>,
    pub name: Option<String>,
    pub health: Option<i32>,
    pub damage: Option<i32>,
    pub armor: Option<i32>,
    pub remaining_health: Option<i32>,
    pub effective_health: Option<i32>,
    pub tavern_tier: Option<u8>,
    pub leaderboard_place: Option<u8>,
}

impl HeroSnapshot {
    pub fn from_entity(entity: &Entity) -> Self {
        Self::from_entity_with_catalog(entity, None)
    }

    pub fn from_entity_with_catalog(entity: &Entity, catalog: Option<&CardCatalog>) -> Self {
        let resolved = entity
            .card_id
            .as_deref()
            .and_then(|card_id| catalog.and_then(|catalog| catalog.resolve(card_id)));
        let health = i32_tag(entity, "HEALTH").or_else(|| resolved.and_then(|c| c.health()));
        let damage = i32_tag(entity, "DAMAGE");
        let armor = i32_tag(entity, "ARMOR");
        let remaining_health = health.map(|h| h - damage.unwrap_or(0));
        let effective_health = remaining_health.map(|h| h + armor.unwrap_or(0));
        let static_name = resolved.and_then(|c| c.preferred_name()).map(str::to_owned);
        let entity_name = trustworthy_entity_name(entity.name.as_deref()).map(str::to_owned);

        Self {
            entity_id: entity.id,
            player_id: i32_tag(entity, "PLAYER_ID"),
            card_id: entity.card_id.clone(),
            name: static_name.or(entity_name),
            health,
            damage,
            armor,
            remaining_health,
            effective_health,
            tavern_tier: i32_tag(entity, "PLAYER_TECH_LEVEL").and_then(|v| u8::try_from(v).ok()),
            leaderboard_place: i32_tag(entity, "PLAYER_LEADERBOARD_PLACE").and_then(|v| u8::try_from(v).ok()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeroPowerSnapshot {
    pub entity_id: u32,
    pub card_id: Option<String>,
    pub name: Option<String>,
    pub text: Option<String>,
    pub cost: Option<i32>,
    pub exhausted: bool,
    pub available: bool,
    /// Forward-compatible script/progress state. This intentionally preserves
    /// relevant live tags instead of hard-coding every hero's mechanic.
    pub dynamic_tags: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LobbyPlayerSnapshot {
    pub player_id: i32,
    pub is_local: bool,
    pub player_name: Option<String>,
    pub hero: HeroSnapshot,
    pub play_state: Option<String>,
    pub alive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecruitSnapshot {
    pub round_number: u32,
    pub raw_game_turn: u32,
    pub timestamp: Option<String>,
    pub hero: Option<HeroSnapshot>,
    pub hero_power: Option<HeroPowerSnapshot>,
    pub gold: Option<i32>,
    pub tavern_tier: Option<u8>,
    pub refresh_cost: Option<i32>,
    pub upgrade_cost: Option<i32>,
    pub shop_frozen: Option<bool>,
    /// TIMEOUT value supplied by Hearthstone at the start of the action window.
    pub timeout_seconds: Option<i32>,
    pub next_opponent_player_id: Option<i32>,
    pub lobby: Vec<LobbyPlayerSnapshot>,
    pub board: Vec<CardSnapshot>,
    pub hand: Vec<CardSnapshot>,
    pub shop: Vec<CardSnapshot>,
    pub trinkets: Vec<CardSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatSideSnapshot {
    pub hero: Option<HeroSnapshot>,
    pub board: Vec<CardSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveCombatSnapshot {
    pub round_number: u32,
    pub raw_game_turn: u32,
    pub timestamp: Option<String>,
    pub opponent_player_id: Option<i32>,
    pub lobby: Vec<LobbyPlayerSnapshot>,
    pub player: CombatSideSnapshot,
    pub opponent: CombatSideSnapshot,
}

pub struct StateProjector<'a> {
    store: &'a EntityStore,
    catalog: Option<&'a CardCatalog>,
}

impl<'a> StateProjector<'a> {
    pub fn new(store: &'a EntityStore) -> Self {
        Self { store, catalog: None }
    }

    pub fn with_catalog(store: &'a EntityStore, catalog: &'a CardCatalog) -> Self {
        Self {
            store,
            catalog: Some(catalog),
        }
    }

    pub fn local_player_id(&self) -> Option<i32> {
        self.store.local_player_id()
    }

    pub fn local_hero(&self) -> Option<HeroSnapshot> {
        let player_id = self.local_player_id()?;
        let entity = self.hero_entity_for_player(player_id, true)?;
        let mut hero = HeroSnapshot::from_entity_with_catalog(entity, self.catalog);
        hero.leaderboard_place = self
            .store
            .leaderboard_place(player_id)
            .and_then(|place| u8::try_from(place).ok())
            .or(hero.leaderboard_place);
        Some(hero)
    }

    pub fn hero_for_player(&self, player_id: i32) -> Option<HeroSnapshot> {
        let entity = self.hero_entity_for_player(player_id, false)?;
        let mut hero = HeroSnapshot::from_entity_with_catalog(entity, self.catalog);
        hero.leaderboard_place = self
            .store
            .leaderboard_place(player_id)
            .and_then(|place| u8::try_from(place).ok())
            .or(hero.leaderboard_place);
        Some(hero)
    }

    pub fn lobby_players(&self) -> Vec<LobbyPlayerSnapshot> {
        let local = self.local_player_id();
        let mut by_player: BTreeMap<i32, &Entity> = BTreeMap::new();

        for entity in self.store.entities() {
            if entity.tag_symbol("CARDTYPE") != Some("HERO") || is_results_hero_clone(entity) {
                continue;
            }
            let Some(player_id) = i32_tag(entity, "PLAYER_ID") else {
                continue;
            };
            if player_id <= 0 || player_id > 8 {
                continue;
            }

            let replace = match by_player.get(&player_id) {
                None => true,
                Some(old) => hero_preference(entity, player_id == local.unwrap_or(-1))
                    > hero_preference(old, player_id == local.unwrap_or(-1)),
            };
            if replace {
                by_player.insert(player_id, entity);
            }
        }

        by_player
            .into_iter()
            .map(|(player_id, hero_entity)| {
                let mut hero = HeroSnapshot::from_entity_with_catalog(hero_entity, self.catalog);
                hero.tavern_tier = self
                    .player_i32(player_id, "PLAYER_TECH_LEVEL")
                    .and_then(|value| u8::try_from(value).ok())
                    .or(hero.tavern_tier);
                hero.leaderboard_place = self
                    .store
                    .leaderboard_place(player_id)
                    .and_then(|place| u8::try_from(place).ok())
                    .or(hero.leaderboard_place);
                let play_state = self.player_symbol(player_id, "PLAYSTATE").map(str::to_owned);
                // PLAYSTATE=WON/TIED can describe a just-finished combat, not lobby elimination.
                // Only LOST (or zero remaining health) is treated as dead.
                let alive = hero.remaining_health.unwrap_or(1) > 0
                    && !matches!(play_state.as_deref(), Some("LOST"));
                LobbyPlayerSnapshot {
                    player_id,
                    is_local: Some(player_id) == local,
                    player_name: self.store.player_name(player_id).map(str::to_owned),
                    hero,
                    play_state,
                    alive,
                }
            })
            .collect()
    }

    pub fn hero_power(&self) -> Option<HeroPowerSnapshot> {
        let local = self.local_player_id()?;
        let entity = self
            .store
            .entities()
            .filter(|entity| {
                i32_tag(entity, "CONTROLLER") == Some(local)
                    && entity.tag_symbol("CARDTYPE") == Some("HERO_POWER")
                    && matches!(entity.tag_symbol("ZONE"), Some("PLAY") | Some("SETASIDE"))
            })
            .max_by_key(|entity| (entity.tag_symbol("ZONE") == Some("PLAY"), entity.id))?;
        let resolved = entity
            .card_id
            .as_deref()
            .and_then(|card_id| self.catalog.and_then(|catalog| catalog.resolve(card_id)));
        let exhausted = truthy(entity, "EXHAUSTED");
        let cant_play = truthy(entity, "CANT_PLAY");
        Some(HeroPowerSnapshot {
            entity_id: entity.id,
            card_id: entity.card_id.clone(),
            name: resolved
                .and_then(|card| card.preferred_name())
                .map(str::to_owned)
                .or_else(|| trustworthy_entity_name(entity.name.as_deref()).map(str::to_owned)),
            text: resolved.and_then(|card| card.preferred_text()).map(str::to_owned),
            cost: i32_tag(entity, "COST").or_else(|| resolved.and_then(|card| card.cost())),
            exhausted,
            available: !exhausted && !cant_play,
            dynamic_tags: selected_dynamic_tags(entity),
        })
    }

    pub fn timeout_seconds(&self) -> Option<i32> {
        let player_id = self.local_player_id()?;
        self.player_i32(player_id, "TIMEOUT").filter(|value| *value > 0)
    }

    pub fn refresh_cost(&self) -> Option<i32> {
        self.button_cost(&["TB_BaconShop_8p_Reroll_Button", "TB_BaconShop_8P_Reroll_Button"])
    }

    pub fn upgrade_cost(&self) -> Option<i32> {
        let current = self.tavern_tier()?;
        let target_tier = current.saturating_add(1);
        self.store
            .entities()
            .filter(|entity| {
                entity
                    .card_id
                    .as_deref()
                    .map(|card_id| card_id.starts_with("TB_BaconShopTechUp"))
                    .unwrap_or(false)
                    && entity.tag_symbol("ZONE") == Some("PLAY")
            })
            .filter(|entity| {
                i32_tag(entity, "TECH_LEVEL") == Some(i32::from(target_tier))
                    || entity
                        .card_id
                        .as_deref()
                        .map(|card_id| card_id.contains(&format!("{:02}", target_tier)))
                        .unwrap_or(false)
            })
            .filter_map(|entity| i32_tag(entity, "COST"))
            .min()
    }

    pub fn shop_frozen(&self) -> Option<bool> {
        let lock = self.store.entities().find(|entity| {
            entity.card_id.as_deref() == Some("TB_BaconShopLockAll_Button")
                && entity.tag_symbol("ZONE") == Some("PLAY")
        });
        if let Some(lock) = lock {
            if lock.tags.contains_key("TAG_SCRIPT_DATA_NUM_1") {
                return Some(truthy(lock, "TAG_SCRIPT_DATA_NUM_1"));
            }
        }
        let Some(dummy) = self.store.dummy_player_id() else {
            return None;
        };
        let mut saw_shop = false;
        let mut frozen = false;
        for entity in self.store.entities().filter(|entity| {
            i32_tag(entity, "CONTROLLER") == Some(dummy)
                && entity.tag_symbol("ZONE") == Some("PLAY")
                && i32_tag(entity, "ZONE_POSITION").unwrap_or(0) > 0
                && self.is_shop_card_entity(entity)
        }) {
            saw_shop = true;
            frozen |= entity.tag_i64("FROZEN").unwrap_or(0) != 0
                || entity.tag_i64("BACON_IS_FROZEN").unwrap_or(0) != 0;
        }
        saw_shop.then_some(frozen)
    }

    pub fn current_gold(&self) -> Option<i32> {
        let player_id = self.local_player_id()?;
        let resources = self.player_i32(player_id, "RESOURCES")?;
        let temp = self.player_i32(player_id, "TEMP_RESOURCES").unwrap_or(0);
        let used = self.player_i32(player_id, "RESOURCES_USED").unwrap_or(0);
        Some((resources + temp - used).max(0))
    }

    pub fn tavern_tier(&self) -> Option<u8> {
        let player_id = self.local_player_id()?;
        self.player_i32(player_id, "PLAYER_TECH_LEVEL")
            .or_else(|| self.local_hero().and_then(|h| h.tavern_tier.map(i32::from)))
            .and_then(|v| u8::try_from(v).ok())
    }

    pub fn next_opponent_player_id(&self) -> Option<i32> {
        let player_id = self.local_player_id()?;
        self.player_i32(player_id, "NEXT_OPPONENT_PLAYER_ID")
    }

    pub fn board(&self) -> Vec<CardSnapshot> {
        let Some(local) = self.local_player_id() else {
            return Vec::new();
        };
        self.sorted_cards(self.store.entities().filter(|e| {
            i32_tag(e, "CONTROLLER") == Some(local)
                && e.tag_symbol("ZONE") == Some("PLAY")
                && e.tag_symbol("CARDTYPE") == Some("MINION")
                && i32_tag(e, "ZONE_POSITION").unwrap_or(0) > 0
        }))
    }

    pub fn hand(&self) -> Vec<CardSnapshot> {
        let Some(local) = self.local_player_id() else {
            return Vec::new();
        };
        self.sorted_cards(self.store.entities().filter(|e| {
            i32_tag(e, "CONTROLLER") == Some(local)
                && e.tag_symbol("ZONE") == Some("HAND")
                && e.card_id.is_some()
                && e.tag_symbol("CARDTYPE") != Some("HERO")
        }))
    }

    pub fn shop(&self) -> Vec<CardSnapshot> {
        let Some(dummy) = self.store.dummy_player_id() else {
            return Vec::new();
        };
        self.sorted_cards(self.store.entities().filter(|e| {
            i32_tag(e, "CONTROLLER") == Some(dummy)
                && e.tag_symbol("ZONE") == Some("PLAY")
                && i32_tag(e, "ZONE_POSITION").unwrap_or(0) > 0
                && self.is_shop_card_entity(e)
        }))
    }

    pub fn trinkets(&self) -> Vec<CardSnapshot> {
        let Some(local) = self.local_player_id() else {
            return Vec::new();
        };
        self.sorted_cards(self.store.entities().filter(|e| {
            if i32_tag(e, "CONTROLLER") != Some(local)
                || e.tag_symbol("ZONE") != Some("PLAY")
                || e.tag_symbol("CARDTYPE") != Some("BATTLEGROUND_TRINKET")
            {
                return false;
            }
            !matches!(
                e.card_id.as_deref(),
                Some("BG30_Trinket_1st") | Some("BG30_Trinket_2nd")
            )
        }))
    }

    pub fn recruit_snapshot(
        &self,
        round_number: u32,
        raw_game_turn: u32,
        timestamp: Option<String>,
    ) -> RecruitSnapshot {
        RecruitSnapshot {
            round_number,
            raw_game_turn,
            timestamp,
            hero: self.local_hero(),
            hero_power: self.hero_power(),
            gold: self.current_gold(),
            tavern_tier: self.tavern_tier(),
            refresh_cost: self.refresh_cost(),
            upgrade_cost: self.upgrade_cost(),
            shop_frozen: self.shop_frozen(),
            timeout_seconds: self.timeout_seconds(),
            next_opponent_player_id: self.next_opponent_player_id(),
            lobby: self.lobby_players(),
            board: self.board(),
            hand: self.hand(),
            shop: self.shop(),
            trinkets: self.trinkets(),
        }
    }

    pub fn combat_snapshot(
        &self,
        round_number: u32,
        raw_game_turn: u32,
        timestamp: Option<String>,
    ) -> LiveCombatSnapshot {
        let opponent_player_id = self.next_opponent_player_id();
        let opponent_hero = opponent_player_id.and_then(|id| self.hero_for_combat_opponent(id));
        LiveCombatSnapshot {
            round_number,
            raw_game_turn,
            timestamp,
            opponent_player_id,
            lobby: self.lobby_players(),
            player: CombatSideSnapshot {
                hero: self.local_hero(),
                board: self.board(),
            },
            opponent: CombatSideSnapshot {
                hero: opponent_hero,
                board: self.opponent_board(),
            },
        }
    }

    fn opponent_board(&self) -> Vec<CardSnapshot> {
        let Some(dummy) = self.store.dummy_player_id() else {
            return Vec::new();
        };
        self.sorted_cards(self.store.entities().filter(|e| {
            i32_tag(e, "CONTROLLER") == Some(dummy)
                && e.tag_symbol("ZONE") == Some("PLAY")
                && e.tag_symbol("CARDTYPE") == Some("MINION")
                && i32_tag(e, "ZONE_POSITION").unwrap_or(0) > 0
        }))
    }

    fn hero_for_combat_opponent(&self, player_id: i32) -> Option<HeroSnapshot> {
        let hero = self
            .store
            .entities()
            .filter(|e| {
                e.tag_symbol("CARDTYPE") == Some("HERO")
                    && i32_tag(e, "PLAYER_ID") == Some(player_id)
                    && e.tag_symbol("ZONE") == Some("PLAY")
                    && !is_results_hero_clone(e)
            })
            .max_by_key(|e| e.id)
            .or_else(|| self.hero_entity_for_player(player_id, false));
        let entity = hero?;
        let mut snapshot = HeroSnapshot::from_entity_with_catalog(entity, self.catalog);
        snapshot.leaderboard_place = self
            .store
            .leaderboard_place(player_id)
            .and_then(|place| u8::try_from(place).ok())
            .or(snapshot.leaderboard_place);
        Some(snapshot)
    }

    fn hero_entity_for_player(&self, player_id: i32, local: bool) -> Option<&Entity> {
        self.store
            .entities()
            .filter(|e| {
                e.tag_symbol("CARDTYPE") == Some("HERO")
                    && i32_tag(e, "PLAYER_ID") == Some(player_id)
                    && !is_results_hero_clone(e)
            })
            .max_by_key(|e| hero_preference(e, local))
    }

    fn player_i32(&self, player_id: i32, tag: &str) -> Option<i32> {
        self.store
            .player_tag(player_id, tag)
            .and_then(|v| v.as_i64())
            .and_then(|v| i32::try_from(v).ok())
    }

    fn player_symbol(&self, player_id: i32, tag: &str) -> Option<&str> {
        self.store.player_tag(player_id, tag).and_then(|value| value.as_str())
    }

    fn is_shop_card_entity(&self, entity: &Entity) -> bool {
        let dynamic = entity
            .tag_symbol("CARDTYPE")
            .filter(|value| !value.eq_ignore_ascii_case("INVALID"));
        let static_type = entity.card_id.as_deref().and_then(|card_id| {
            self.catalog
                .and_then(|catalog| catalog.resolve(card_id))
                .and_then(|card| card.card_type())
        });
        entity.card_id.as_deref().map(|id| !id.is_empty()).unwrap_or(false)
            && is_shop_card_type(dynamic.or(static_type))
    }

    fn button_cost(&self, card_ids: &[&str]) -> Option<i32> {
        self.store
            .entities()
            .filter(|entity| {
                entity
                    .card_id
                    .as_deref()
                    .map(|card_id| card_ids.contains(&card_id))
                    .unwrap_or(false)
                    && entity.tag_symbol("ZONE") == Some("PLAY")
            })
            .filter_map(|entity| i32_tag(entity, "COST"))
            .min()
    }

    fn sorted_cards<'b>(&self, items: impl Iterator<Item = &'b Entity>) -> Vec<CardSnapshot> {
        let mut cards: Vec<CardSnapshot> = items
            .map(|entity| CardSnapshot::from_entity_with_catalog(entity, self.catalog))
            .collect();
        cards.sort_by_key(|c| (c.zone_position.unwrap_or(i32::MAX), c.entity_id));
        cards
    }
}

fn is_shop_card_type(card_type: Option<&str>) -> bool {
    matches!(
        card_type,
        Some("MINION") | Some("BATTLEGROUND_SPELL") | Some("SPELL")
    )
}

fn trustworthy_entity_name(name: Option<&str>) -> Option<&str> {
    let name = name?.trim();
    if name.is_empty()
        || name.eq_ignore_ascii_case("unknown")
        || name.to_ascii_uppercase().starts_with("UNKNOWN ENTITY")
    {
        None
    } else {
        Some(name)
    }
}

fn is_results_hero_clone(entity: &Entity) -> bool {
    entity
        .tag_i64("BACON_PLAYER_RESULTS_HERO_OVERRIDE")
        .unwrap_or(0)
        != 0
}

fn hero_preference(entity: &Entity, local: bool) -> (i32, u32) {
    let zone_score = match (local, entity.tag_symbol("ZONE")) {
        (true, Some("PLAY")) => 4,
        (false, Some("SETASIDE")) => 4,
        (_, Some("PLAY")) => 3,
        (_, Some("SETASIDE")) => 2,
        _ => 1,
    };
    (zone_score, entity.id)
}

fn has_mechanic(mechanics: &[String], wanted: &str) -> bool {
    fn normalized(value: &str) -> String {
        value
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .flat_map(|ch| ch.to_uppercase())
            .collect()
    }
    let wanted = normalized(wanted);
    mechanics.iter().any(|value| normalized(value) == wanted)
}

fn selected_dynamic_tags(entity: &Entity) -> BTreeMap<String, String> {
    entity
        .tags
        .iter()
        .filter(|(tag, _)| {
            tag.starts_with("TAG_SCRIPT_DATA_")
                || tag.starts_with("BACON_")
                || tag.starts_with("NUM_")
                || matches!(
                    tag.as_str(),
                    "COST"
                        | "EXHAUSTED"
                        | "CANT_PLAY"
                        | "HAS_ACTIVATE_POWER"
                        | "INTERACTABLE_OBJECT"
                        | "INTERACTABLE_OBJECT_COST"
                        | "INTERACTABLE_OBJECT_PASSIVE_ANIMATION_TYPE"
                        | "WAS_EVER_AN_INTERACTABLE_OBJECT"
                        | "TIMEOUT"
                        | "PLAYER_TECH_LEVEL"
                )
        })
        .map(|(tag, value)| {
            let value = value
                .as_i64()
                .map(|value| value.to_string())
                .or_else(|| value.as_str().map(str::to_owned))
                .unwrap_or_default();
            (tag.clone(), value)
        })
        .collect()
}

fn i32_tag(entity: &Entity, tag: &str) -> Option<i32> {
    entity.tag_i64(tag).and_then(|v| i32::try_from(v).ok())
}

fn truthy(entity: &Entity, tag: &str) -> bool {
    entity.tag_i64(tag).unwrap_or(0) != 0
}
