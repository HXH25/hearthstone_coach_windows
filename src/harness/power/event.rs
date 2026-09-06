pub type EntityId = u32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagValue {
    Int(i64),
    Symbol(String),
}

impl TagValue {
    pub fn parse(raw: &str) -> Self {
        let raw = raw.trim();
        match raw.parse::<i64>() {
            Ok(value) => Self::Int(value),
            Err(_) => Self::Symbol(raw.to_owned()),
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            Self::Symbol(_) => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Int(_) => None,
            Self::Symbol(value) => Some(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EntityIdentity {
    pub id: EntityId,
    pub name: Option<String>,
    pub card_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityDescriptor {
    pub identity: EntityIdentity,
    pub zone: Option<String>,
    pub zone_position: Option<i32>,
    pub controller: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagTarget {
    Entity(EntityIdentity),
    GameEntity,
    PlayerName(String),
}

/// Bottom-layer facts only. Battlegrounds semantics such as Buy/Refresh are
/// intentionally inferred later by `HarnessRuntime`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerEvent {
    CreateGame,

    GameEntityDeclared {
        entity_id: EntityId,
    },

    PlayerDeclared {
        entity_id: EntityId,
        player_id: i32,
        has_real_account: bool,
    },

    PlayerNameDeclared {
        player_id: i32,
        name: String,
    },

    /// `FULL_ENTITY - Creating` replaces an old entity with the same id.
    EntityCreated {
        id: EntityId,
        card_id: Option<String>,
    },

    EntityDefined(EntityDescriptor),

    EntityCardChanged {
        identity: EntityIdentity,
        new_card_id: String,
    },

    TagChanged {
        target: TagTarget,
        tag: String,
        value: TagValue,
    },

    /// Direct player input reported by GameState.SendOption(). This is not a
    /// canonical entity mutation, but it is the strongest available boundary
    /// for distinguishing a user action from asynchronous/nested Power blocks.
    UserOptionSent {
        selected_option: i32,
        selected_target: Option<EntityId>,
        selected_position: Option<i32>,
    },

    /// `Entity` and `Target` are deliberately parsed independently. A single
    /// BLOCK_START line often contains two complete descriptors and allowing a
    /// descriptor parser to scan across both silently swaps Buy/Sell sources.
    BlockStarted {
        block_type: String,
        source: Option<EntityIdentity>,
        target: Option<EntityIdentity>,
        effect_card_id: Option<String>,
    },

    BlockEnded,
}
