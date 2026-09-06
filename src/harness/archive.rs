use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::harness::state::{CardSnapshot, CombatSideSnapshot, LobbyPlayerSnapshot, RecruitSnapshot};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChoiceKind {
    Hero,
    Trinket,
    DarkGift,
    Discover,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceArchive {
    pub id: u32,
    pub choice_type: String,
    pub kind: ChoiceKind,
    pub started_at: Option<String>,
    pub resolved_at: Option<String>,
    pub source_name: Option<String>,
    pub source_card_id: Option<String>,
    pub options: Vec<CardSnapshot>,
    pub selected: Vec<CardSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecruitActionKind {
    Refresh,
    BuyMinion,
    BuySpell,
    SellMinion,
    PlayMinion,
    CastSpell,
    TavernUpgrade,
    FreezeToggle,
    UseHeroPower,
    Reorder,
    Activate,
    RecruitAttack,
    SpecialAction,
    PlayCard,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecruitActionRecord {
    pub timestamp: Option<String>,
    pub kind: RecruitActionKind,
    pub source: Option<CardSnapshot>,
    pub target: Option<CardSnapshot>,
    /// State after the action has fully settled, captured at the next
    /// GameState.SendOption or Recruit phase boundary.
    pub state_after: RecruitSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShopRevisionCause {
    PhaseStart,
    Refresh,
    Buy,
    Sell,
    SpellOrEffect,
    OtherAction,
    StateChange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShopRevision {
    /// 1-based within one Recruit phase.
    pub revision: u32,
    pub timestamp: Option<String>,
    pub cause: ShopRevisionCause,
    pub cards: Vec<CardSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecruitArchive {
    pub round_number: u32,
    pub raw_game_turn: u32,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub start: RecruitSnapshot,
    pub actions: Vec<RecruitActionRecord>,
    pub shop_revisions: Vec<ShopRevision>,
    pub choices: Vec<ChoiceArchive>,
    pub end: RecruitSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CombatResult {
    Win,
    Loss,
    Tie,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatArchive {
    pub round_number: u32,
    pub raw_game_turn: u32,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub opponent_player_id: Option<i32>,
    pub lobby: Vec<LobbyPlayerSnapshot>,
    pub player: CombatSideSnapshot,
    pub opponent: CombatSideSnapshot,
    pub result: CombatResult,
    pub damage_taken: Option<i32>,
    pub damage_dealt: Option<i32>,
    pub choices: Vec<ChoiceArchive>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundArchive {
    pub round_number: u32,
    pub recruit: Option<RecruitArchive>,
    pub combat: Option<CombatArchive>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameArchive {
    pub schema_version: String,
    pub source_log: Option<String>,
    /// Audit field: where CardId -> static metadata came from.
    pub card_catalog_source: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub local_player_id: Option<i32>,
    pub local_player_name: Option<String>,
    pub final_place: Option<u8>,
    /// Optional client-visible tribe pool. Live CLI can enrich this through the
    /// installed HDT HearthMirror provider; pure Power.log/replay remains independent.
    pub available_tribes: Option<Vec<String>>,
    pub available_tribes_source: Option<String>,
    /// Defensive parser/runtime recoveries from an unbalanced block stack.
    /// Zero is ideal; non-zero means recovery occurred but did not poison the
    /// rest of the match.
    #[serde(default)]
    pub stale_block_stack_recoveries: u32,
    pub lobby_players: Vec<LobbyPlayerSnapshot>,
    pub pregame_choices: Vec<ChoiceArchive>,
    /// Exactly one object per Battlegrounds round. Each object contains up to
    /// two sub-phases: Recruit and Combat.
    pub rounds: Vec<RoundArchive>,
}

impl Default for GameArchive {
    fn default() -> Self {
        Self {
            schema_version: "0.4.2".to_owned(),
            source_log: None,
            card_catalog_source: None,
            started_at: None,
            ended_at: None,
            local_player_id: None,
            local_player_name: None,
            final_place: None,
            available_tribes: None,
            available_tribes_source: None,
            stale_block_stack_recoveries: 0,
            lobby_players: Vec::new(),
            pregame_choices: Vec::new(),
            rounds: Vec::new(),
        }
    }
}

impl GameArchive {
    pub fn round(&self, round_number: u32) -> Option<&RoundArchive> {
        self.rounds.iter().find(|r| r.round_number == round_number)
    }

    pub fn latest_round(&self) -> Option<&RoundArchive> {
        self.rounds.last()
    }

    pub fn local_player(&self) -> Option<&LobbyPlayerSnapshot> {
        self.lobby_players.iter().find(|p| p.is_local)
    }

    pub fn opponent(&self, player_id: i32) -> Option<&LobbyPlayerSnapshot> {
        self.lobby_players.iter().find(|p| p.player_id == player_id)
    }

    pub fn choices(&self) -> impl Iterator<Item = &ChoiceArchive> {
        self.pregame_choices
            .iter()
            .chain(self.rounds.iter().flat_map(|round| {
                round
                    .recruit
                    .iter()
                    .flat_map(|r| r.choices.iter())
                    .chain(round.combat.iter().flat_map(|c| c.choices.iter()))
            }))
    }

    pub fn shop_revisions(&self, round_number: u32) -> &[ShopRevision] {
        self.round(round_number)
            .and_then(|round| round.recruit.as_ref())
            .map(|recruit| recruit.shop_revisions.as_slice())
            .unwrap_or(&[])
    }

    pub fn lobby_at_round(&self, round_number: u32) -> Option<&[LobbyPlayerSnapshot]> {
        self.round(round_number)
            .and_then(|round| round.recruit.as_ref())
            .map(|recruit| recruit.start.lobby.as_slice())
            .or_else(|| self.round(round_number).and_then(|round| round.combat.as_ref()).map(|combat| combat.lobby.as_slice()))
    }

    pub fn latest_stable_shop(&self, round_number: u32) -> Option<&[CardSnapshot]> {
        self.shop_revisions(round_number)
            .last()
            .map(|revision| revision.cards.as_slice())
    }

    /// All archived combats against one lobby player, in chronological order.
    pub fn combats_against(
        &self,
        player_id: i32,
    ) -> impl Iterator<Item = &CombatArchive> {
        self.rounds
            .iter()
            .filter_map(|round| round.combat.as_ref())
            .filter(move |combat| combat.opponent_player_id == Some(player_id))
    }
}

#[derive(Debug, Clone)]
pub struct ArchivePaths {
    pub json: PathBuf,
    pub markdown: PathBuf,
}

pub fn write_archive_files(archive: &GameArchive, output_dir: &Path) -> io::Result<ArchivePaths> {
    fs::create_dir_all(output_dir)?;
    let base = archive_basename(archive);
    let json_path = output_dir.join(format!("{base}.json"));
    let markdown_path = output_dir.join(format!("{base}.md"));

    let json_file = File::create(&json_path)?;
    serde_json::to_writer_pretty(json_file, archive)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let mut md = File::create(&markdown_path)?;
    md.write_all(render_markdown(archive).as_bytes())?;

    Ok(ArchivePaths {
        json: json_path,
        markdown: markdown_path,
    })
}

fn archive_basename(archive: &GameArchive) -> String {
    if let Some(source) = &archive.source_log {
        let path = Path::new(source);
        if let Some(parent) = path.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()) {
            return format!("hearthcoach_{parent}");
        }
    }
    "hearthcoach_match".to_owned()
}

fn render_markdown(archive: &GameArchive) -> String {
    let mut out = String::new();
    out.push_str("# HearthCoach Battlegrounds Match Log\n\n");
    out.push_str(&format!("- Schema: `{}`\n", archive.schema_version));
    out.push_str(&format!("- Source: `{}`\n", archive.source_log.as_deref().unwrap_or("unknown")));
    out.push_str(&format!(
        "- Card Catalog: `{}`\n",
        archive.card_catalog_source.as_deref().unwrap_or("none")
    ));
    out.push_str(&format!("- Start: `{}`\n", archive.started_at.as_deref().unwrap_or("unknown")));
    out.push_str(&format!("- End: `{}`\n", archive.ended_at.as_deref().unwrap_or("unknown")));
    out.push_str(&format!("- Local Player ID: `{:?}`\n", archive.local_player_id));
    out.push_str(&format!("- Local Player: `{}`\n", archive.local_player_name.as_deref().unwrap_or("unknown")));
    out.push_str(&format!("- Final Place: `{:?}`\n", archive.final_place));
    out.push_str(&format!("- Available Tribes: `{:?}`\n", archive.available_tribes));
    out.push_str(&format!(
        "- Available Tribes Source: `{}`\n",
        archive.available_tribes_source.as_deref().unwrap_or("none")
    ));
    out.push_str(&format!(
        "- Block Stack Recoveries: `{}`\n\n",
        archive.stale_block_stack_recoveries
    ));

    out.push_str("## Lobby\n\n");
    for p in &archive.lobby_players {
        out.push_str(&format!(
            "- P{}{}: {} (`{}`), place={:?}\n",
            p.player_id,
            if p.is_local { " [YOU]" } else { "" },
            p.hero.name.as_deref().unwrap_or("unknown hero"),
            p.hero.card_id.as_deref().unwrap_or("unknown"),
            p.hero.leaderboard_place
        ));
    }

    if !archive.pregame_choices.is_empty() {
        out.push_str("\n## Pregame Choices\n\n");
        for choice in &archive.pregame_choices {
            render_choice(&mut out, choice);
        }
    }

    for round in &archive.rounds {
        out.push_str(&format!("\n## Round {}\n", round.round_number));
        if let Some(recruit) = &round.recruit {
            out.push_str("\n### Recruit\n\n");
            out.push_str(&format!("- Time: {:?} -> {:?}\n", recruit.started_at, recruit.ended_at));
            out.push_str(&format!("- Start gold/tier: {:?} / {:?}\n", recruit.start.gold, recruit.start.tavern_tier));
            out.push_str(&format!("- End gold/tier: {:?} / {:?}\n", recruit.end.gold, recruit.end.tavern_tier));
            out.push_str(&format!(
                "- Start refresh/upgrade cost: {:?} / {:?}\n",
                recruit.start.refresh_cost, recruit.start.upgrade_cost
            ));
            out.push_str(&format!(
                "- End refresh/upgrade cost: {:?} / {:?}\n",
                recruit.end.refresh_cost, recruit.end.upgrade_cost
            ));
            out.push_str(&format!("- Shop frozen: {:?} -> {:?}\n", recruit.start.shop_frozen, recruit.end.shop_frozen));
            out.push_str(&format!("- Action window TIMEOUT: {:?} seconds\n", recruit.start.timeout_seconds));
            out.push_str(&format!("- Next opponent: {:?}\n", recruit.end.next_opponent_player_id));
            if let Some(hero) = &recruit.start.hero {
                out.push_str(&format!(
                    "- Player HP/Armor: {:?}/{:?} (effective={:?})\n",
                    hero.remaining_health, hero.armor, hero.effective_health
                ));
            }
            if let Some(power) = &recruit.start.hero_power {
                out.push_str(&format!(
                    "- Hero Power: {} (`{}`), cost={:?}, available={}, exhausted={}\n",
                    power.name.as_deref().unwrap_or("unknown"),
                    power.card_id.as_deref().unwrap_or("unknown"),
                    power.cost, power.available, power.exhausted
                ));
            }
            render_lobby_snapshot(&mut out, &recruit.start.lobby);
            render_cards(&mut out, "Start Board", &recruit.start.board);
            render_cards(&mut out, "Start Shop", &recruit.start.shop);

            if !recruit.actions.is_empty() {
                out.push_str("\n#### Actions\n\n");
                for (i, action) in recruit.actions.iter().enumerate() {
                    out.push_str(&format!(
                        "{}. `{:?}` source={} target={} gold_after={:?}\n",
                        i + 1,
                        action.kind,
                        card_label(action.source.as_ref()),
                        card_label(action.target.as_ref()),
                        action.state_after.gold
                    ));
                }
            }

            if !recruit.shop_revisions.is_empty() {
                out.push_str("\n#### Shop Revisions\n\n");
                for revision in &recruit.shop_revisions {
                    let cards = revision
                        .cards
                        .iter()
                        .map(|card| card_label(Some(card)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    out.push_str(&format!(
                        "- #{} `{:?}` at {:?}: [{}]\n",
                        revision.revision, revision.cause, revision.timestamp, cards
                    ));
                }
            }

            if !recruit.choices.is_empty() {
                out.push_str("\n#### Choices\n\n");
                for choice in &recruit.choices {
                    render_choice(&mut out, choice);
                }
            }

            render_cards(&mut out, "End Board", &recruit.end.board);
            render_cards(&mut out, "End Hand", &recruit.end.hand);
            render_cards(&mut out, "End Shop", &recruit.end.shop);
            render_cards(&mut out, "Trinkets", &recruit.end.trinkets);
        }

        if let Some(combat) = &round.combat {
            out.push_str("\n### Combat\n\n");
            out.push_str(&format!("- Time: {:?} -> {:?}\n", combat.started_at, combat.ended_at));
            out.push_str(&format!("- Opponent Player ID: {:?}\n", combat.opponent_player_id));
            out.push_str(&format!("- Result: `{:?}`\n", combat.result));
            out.push_str(&format!("- Damage taken/dealt: {:?} / {:?}\n", combat.damage_taken, combat.damage_dealt));
            out.push_str(&format!("- Player hero: {}\n", hero_label(combat.player.hero.as_ref())));
            out.push_str(&format!("- Opponent hero: {}\n", hero_label(combat.opponent.hero.as_ref())));
            render_lobby_snapshot(&mut out, &combat.lobby);
            render_cards(&mut out, "Player Board at Combat Start", &combat.player.board);
            render_cards(&mut out, "Opponent Board at Combat Start", &combat.opponent.board);
        }
    }

    out
}

fn render_lobby_snapshot(out: &mut String, players: &[LobbyPlayerSnapshot]) {
    if players.is_empty() {
        return;
    }
    out.push_str("\n#### Lobby Snapshot\n\n");
    for player in players {
        out.push_str(&format!(
            "- P{}{} {}: HP={:?} Armor={:?} Tier={:?} Place={:?} Alive={}\n",
            player.player_id,
            if player.is_local { " [YOU]" } else { "" },
            player.hero.name.as_deref().unwrap_or("unknown hero"),
            player.hero.remaining_health,
            player.hero.armor,
            player.hero.tavern_tier,
            player.hero.leaderboard_place,
            player.alive
        ));
    }
}

fn render_choice(out: &mut String, choice: &ChoiceArchive) {
    out.push_str(&format!(
        "- Choice #{} `{:?}` source={} options=[{}] selected=[{}]\n",
        choice.id,
        choice.kind,
        choice.source_name.as_deref().or(choice.source_card_id.as_deref()).unwrap_or("GameEntity"),
        choice.options.iter().map(|c| card_label(Some(c))).collect::<Vec<_>>().join(", "),
        choice.selected.iter().map(|c| card_label(Some(c))).collect::<Vec<_>>().join(", ")
    ));
}

fn render_cards(out: &mut String, title: &str, cards: &[CardSnapshot]) {
    out.push_str(&format!("\n#### {title}\n\n"));
    if cards.is_empty() {
        out.push_str("- (empty)\n");
        return;
    }
    for card in cards {
        let mut keywords = Vec::new();
        if card.keywords.taunt { keywords.push("TAUNT"); }
        if card.keywords.divine_shield { keywords.push("DIVINE_SHIELD"); }
        if card.keywords.reborn { keywords.push("REBORN"); }
        if card.keywords.poisonous { keywords.push("POISONOUS"); }
        if card.keywords.venomous { keywords.push("VENOMOUS"); }
        if card.keywords.windfury { keywords.push("WINDFURY"); }
        if card.keywords.mega_windfury { keywords.push("MEGA_WINDFURY"); }
        if card.keywords.stealth { keywords.push("STEALTH"); }
        if card.keywords.battlecry { keywords.push("BATTLECRY"); }
        if card.keywords.deathrattle { keywords.push("DEATHRATTLE"); }
        if card.keywords.activate_keyword { keywords.push("ACTIVATE"); }
        if card.keywords.activate_available_now { keywords.push("ACTIVATE_AVAILABLE"); }
        out.push_str(&format!(
            "- [{}] {} (`{}`) {}/{} tier={:?} printed_cost={:?} golden={} tribes={:?} keywords={:?}\n",
            card.zone_position.map(|x| x.to_string()).unwrap_or_else(|| "-".to_owned()),
            card.name.as_deref().unwrap_or("unknown"),
            card.card_id.as_deref().unwrap_or("unknown"),
            card.attack.map(|x| x.to_string()).unwrap_or_else(|| "-".to_owned()),
            card.remaining_health.map(|x| x.to_string()).unwrap_or_else(|| "-".to_owned()),
            card.tavern_tier,
            card.printed_cost,
            card.is_golden,
            card.tribes,
            keywords
        ));
    }
}

fn card_label(card: Option<&CardSnapshot>) -> String {
    let Some(card) = card else { return "-".to_owned(); };
    format!(
        "{}({})",
        card.name.as_deref().unwrap_or("unknown"),
        card.card_id.as_deref().unwrap_or("unknown")
    )
}

fn hero_label(hero: Option<&crate::harness::state::HeroSnapshot>) -> String {
    let Some(hero) = hero else { return "unknown".to_owned(); };
    format!(
        "{}({}) effective_hp={:?}",
        hero.name.as_deref().unwrap_or("unknown"),
        hero.card_id.as_deref().unwrap_or("unknown"),
        hero.effective_health
    )
}
