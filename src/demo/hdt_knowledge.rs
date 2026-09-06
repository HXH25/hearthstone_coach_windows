use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::harness::{CardCatalog, CardMeta};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KnowledgeStats {
    pub source: String,
    pub total_cards: usize,
    pub bacon_pool_minions: usize,
}

/// Current Battlegrounds factual knowledge backed by HDT's installed
/// HearthDb.dll + downloaded CardDefs.base.xml.
///
/// DeepSeek is deliberately not a source of card facts. This layer decides
/// which minions are currently in the pool and supplies CardId/name/tier/text
/// to the model. Model output is validated against the same layer afterward.
#[derive(Debug, Clone)]
pub struct HdtKnowledgeBase {
    catalog: CardCatalog,
    stats: KnowledgeStats,
}

#[derive(Debug, Clone)]
pub struct KnowledgeCard {
    pub card_id: String,
    pub name: String,
    pub tavern_tier: u8,
    pub tribes: Vec<String>,
    pub text: String,
    pub mechanics: Vec<String>,
}

impl HdtKnowledgeBase {
    pub fn from_catalog(catalog: CardCatalog) -> Self {
        let bacon_pool_minions = catalog
            .cards()
            .filter(|card| card.in_bacon_pool && !card.premium)
            .count();
        let stats = KnowledgeStats {
            source: catalog.source().unwrap_or("unknown").to_owned(),
            total_cards: catalog.len(),
            bacon_pool_minions,
        };
        Self { catalog, stats }
    }

    pub fn stats(&self) -> &KnowledgeStats {
        &self.stats
    }

    pub fn catalog(&self) -> &CardCatalog {
        &self.catalog
    }

    /// Return every *current* normal Battlegrounds pool minion that can appear
    /// under this match's available-tribe restriction. Neutral minions are
    /// retained. The result is deterministic and is never arbitrarily truncated.
    pub fn current_pool(&self, available_tribes: &[String]) -> Vec<KnowledgeCard> {
        let allowed = normalized_tribes(available_tribes);
        let mut cards = self
            .catalog
            .cards()
            .filter(|card| is_current_normal_pool_minion(card))
            .filter_map(|card| {
                let resolved = self.catalog.resolve(&card.card_id)?;
                let tribes = resolved.tribes();
                if !tribes_allowed(&tribes, &allowed) {
                    return None;
                }
                Some(KnowledgeCard {
                    card_id: card.card_id.clone(),
                    name: resolved.preferred_name().unwrap_or(&card.card_id).to_owned(),
                    tavern_tier: resolved.tavern_tier()?,
                    tribes,
                    text: sanitize(resolved.preferred_text().unwrap_or("")),
                    mechanics: resolved.mechanics(),
                })
            })
            .collect::<Vec<_>>();
        cards.sort_by(|a, b| {
            (a.tavern_tier, a.name.as_str(), a.card_id.as_str())
                .cmp(&(b.tavern_tier, b.name.as_str(), b.card_id.as_str()))
        });
        cards
    }

    pub fn current_pool_count(&self, available_tribes: &[String]) -> usize {
        let allowed = normalized_tribes(available_tribes);
        self.catalog
            .cards()
            .filter(|card| is_current_normal_pool_minion(card))
            .filter(|card| {
                self.catalog
                    .resolve(&card.card_id)
                    .map(|resolved| tribes_allowed(&resolved.tribes(), &allowed))
                    .unwrap_or(false)
            })
            .count()
    }

    /// Verify a normal/premium CardId against the current HDT pool and this
    /// match's tribes. Returns the canonical normal pool CardId on success.
    pub fn validate_pool_card_id(&self, card_id: &str, available_tribes: &[String]) -> Option<String> {
        let resolved = self.catalog.resolve(card_id)?;
        let canonical = resolved
            .normal_card_id()
            .filter(|id| self.catalog.get(id).is_some())
            .unwrap_or(card_id);
        let card = self.catalog.get(canonical)?;
        if !is_current_normal_pool_minion(card) {
            return None;
        }
        let tribes = self.catalog.resolve(canonical)?.tribes();
        let allowed = normalized_tribes(available_tribes);
        if !tribes_allowed(&tribes, &allowed) {
            return None;
        }
        Some(canonical.to_owned())
    }

    /// Compact factual context for DeepSeek. Every line comes from the current
    /// HDT CardDefs snapshot; there is intentionally no arbitrary first-N cut.
    pub fn prompt_context(&self, available_tribes: &[String]) -> String {
        self.current_pool(available_tribes)
            .into_iter()
            .map(|card| {
                let mechanics = if card.mechanics.is_empty() {
                    String::new()
                } else {
                    format!(" | mechanics={}", card.mechanics.join(","))
                };
                format!(
                    "{} | {} | T{} | {} | {}{}",
                    card.card_id,
                    card.name,
                    card.tavern_tier,
                    if card.tribes.is_empty() { "NEUTRAL".to_owned() } else { card.tribes.join("/") },
                    card.text,
                    mechanics,
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn is_current_normal_pool_minion(card: &CardMeta) -> bool {
    card.in_bacon_pool
        && !card.premium
        && card.tavern_tier.unwrap_or(0) > 0
        && card.card_type.as_deref().map(|value| value.eq_ignore_ascii_case("MINION")) == Some(true)
}

fn normalized_tribes(available_tribes: &[String]) -> BTreeSet<String> {
    available_tribes
        .iter()
        .map(|tribe| tribe.trim().to_ascii_uppercase())
        .collect()
}

fn tribes_allowed(tribes: &[String], allowed: &BTreeSet<String>) -> bool {
    // No race tag means a neutral minion. If tribe discovery has not completed
    // yet, preserve the whole current pool rather than guessing.
    tribes.is_empty() || allowed.is_empty() || tribes.iter().any(|tribe| allowed.contains(&tribe.to_ascii_uppercase()))
}

fn sanitize(text: &str) -> String {
    text.replace(['\r', '\n', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
