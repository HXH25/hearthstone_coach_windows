use crate::harness::power::EntityDescriptor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceEvent {
    Opened {
        id: u32,
        player_name: String,
        choice_type: String,
        count_min: usize,
        count_max: usize,
    },
    Source {
        id: u32,
        source: Option<EntityDescriptor>,
        raw_source: String,
    },
    Option {
        id: u32,
        index: usize,
        entity: EntityDescriptor,
    },
    SelectionStarted {
        id: u32,
        entities_count: usize,
    },
    Selected {
        id: u32,
        index: usize,
        entity: EntityDescriptor,
    },
}
