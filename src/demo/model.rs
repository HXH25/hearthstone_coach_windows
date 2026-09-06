use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CompositionOption {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub difficulty: String,
    #[serde(default)]
    pub key_tribes: Vec<String>,
    /// Current HDT pool cards used as factual evidence for this composition.
    #[serde(default)]
    pub core_card_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CompositionResponse {
    #[serde(default)]
    pub compositions: Vec<CompositionOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WatchCard {
    pub card_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub tavern_tier: Option<u8>,
    pub priority: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StageWatchlist {
    pub stage: String,
    #[serde(default)]
    pub cards: Vec<WatchCard>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WatchlistResponse {
    #[serde(default)]
    pub composition_id: String,
    #[serde(default)]
    pub stages: Vec<StageWatchlist>,
}

impl WatchlistResponse {
    pub fn find_card(&self, card_id: &str) -> Option<(&str, &WatchCard)> {
        for stage in &self.stages {
            if let Some(card) = stage.cards.iter().find(|card| card.card_id == card_id) {
                return Some((stage.stage.as_str(), card));
            }
        }
        None
    }

    pub fn find_card_for_stage(&self, stage_name: &str, card_id: &str) -> Option<(&str, &WatchCard)> {
        let stage = self.stages.iter().find(|stage| stage.stage == stage_name)?;
        stage
            .cards
            .iter()
            .find(|card| card.card_id == card_id)
            .map(|card| (stage.stage.as_str(), card))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ShopHit {
    pub shop_index: usize,
    pub card_id: String,
    pub name: String,
    #[serde(default)]
    pub tavern_tier: Option<u8>,
    pub stage: String,
    pub priority: String,
    pub role: String,
    pub reason: String,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiUsage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    /// Provider-reported cached prompt tokens when available (DeepSeek-compatible).
    #[serde(default)]
    pub prompt_cache_hit_tokens: u64,
    #[serde(default)]
    pub prompt_cache_miss_tokens: u64,
}

// -----------------------------------------------------------------------------
// V0.4.5 in-game coach chat / user strategy correction
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatMessage {
    /// `user` / `assistant` / `system`.
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub content: String,
    /// True when this assistant turn changed the live RoundPlan.
    #[serde(default)]
    pub plan_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EconomyPolicyPatch {
    #[serde(default)]
    pub max_rerolls: Option<u8>,
    #[serde(default)]
    pub gold_reserve: Option<i32>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TierPolicyPatch {
    #[serde(default)]
    pub posture: Option<UpgradePosture>,
    #[serde(default)]
    pub target_tier: Option<u8>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DecisionWeightsPatch {
    #[serde(default)]
    pub tempo: Option<f32>,
    #[serde(default)]
    pub scaling: Option<f32>,
    #[serde(default)]
    pub economy: Option<f32>,
    #[serde(default)]
    pub synergy: Option<f32>,
    #[serde(default)]
    pub triple: Option<f32>,
    #[serde(default)]
    pub survival: Option<f32>,
}

/// A deliberately partial patch. Chat is allowed to correct the current plan
/// without replacing the whole object or changing its target round.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoundPlanPatch {
    #[serde(default)]
    pub primary_goal: Option<String>,
    #[serde(default)]
    pub secondary_goals: Option<Vec<String>>,
    #[serde(default)]
    pub direction: Option<Vec<String>>,
    #[serde(default)]
    pub economy: Option<EconomyPolicyPatch>,
    #[serde(default)]
    pub tier_policy: Option<TierPolicyPatch>,
    #[serde(default)]
    pub weights: Option<DecisionWeightsPatch>,
    #[serde(default)]
    pub replan_conditions: Option<Vec<String>>,
    #[serde(default)]
    pub explanation: Option<String>,
}

impl RoundPlanPatch {
    pub fn is_empty(&self) -> bool {
        self.primary_goal.is_none()
            && self.secondary_goals.is_none()
            && self.direction.is_none()
            && self.economy.is_none()
            && self.tier_policy.is_none()
            && self.weights.is_none()
            && self.replan_conditions.is_none()
            && self.explanation.is_none()
    }

    /// Merge a user-approved/AI-interpreted correction into the live plan.
    /// Returns true only if at least one field was actually changed.
    pub fn apply_to(&self, plan: &mut RoundPlan) -> bool {
        let mut changed = false;

        if let Some(value) = clean_optional_text(self.primary_goal.as_deref()) {
            if plan.primary_goal != value {
                plan.primary_goal = value;
                changed = true;
            }
        }
        if let Some(values) = self.secondary_goals.as_ref() {
            let values = clean_text_list(values, 4);
            if plan.secondary_goals != values {
                plan.secondary_goals = values;
                changed = true;
            }
        }
        if let Some(values) = self.direction.as_ref() {
            let values = clean_text_list(values, 4);
            if plan.direction != values {
                plan.direction = values;
                changed = true;
            }
        }
        if let Some(economy) = self.economy.as_ref() {
            if let Some(value) = economy.max_rerolls {
                let value = value.min(8);
                if plan.economy.max_rerolls != value {
                    plan.economy.max_rerolls = value;
                    changed = true;
                }
            }
            if let Some(value) = economy.gold_reserve {
                let value = value.max(0);
                if plan.economy.gold_reserve != value {
                    plan.economy.gold_reserve = value;
                    changed = true;
                }
            }
            if let Some(value) = clean_optional_text(economy.reason.as_deref()) {
                if plan.economy.reason != value {
                    plan.economy.reason = value;
                    changed = true;
                }
            }
        }
        if let Some(tier) = self.tier_policy.as_ref() {
            if let Some(value) = tier.posture {
                if plan.tier_policy.posture != value {
                    plan.tier_policy.posture = value;
                    changed = true;
                }
            }
            if let Some(value) = tier.target_tier {
                let value = value.clamp(1, 6);
                if plan.tier_policy.target_tier != Some(value) {
                    plan.tier_policy.target_tier = Some(value);
                    changed = true;
                }
            }
            if let Some(value) = clean_optional_text(tier.reason.as_deref()) {
                if plan.tier_policy.reason != value {
                    plan.tier_policy.reason = value;
                    changed = true;
                }
            }
        }
        if let Some(weights) = self.weights.as_ref() {
            changed |= patch_weight(&mut plan.weights.tempo, weights.tempo);
            changed |= patch_weight(&mut plan.weights.scaling, weights.scaling);
            changed |= patch_weight(&mut plan.weights.economy, weights.economy);
            changed |= patch_weight(&mut plan.weights.synergy, weights.synergy);
            changed |= patch_weight(&mut plan.weights.triple, weights.triple);
            changed |= patch_weight(&mut plan.weights.survival, weights.survival);
        }
        if let Some(values) = self.replan_conditions.as_ref() {
            let values = clean_text_list(values, 6);
            if plan.replan_conditions != values {
                plan.replan_conditions = values;
                changed = true;
            }
        }
        if let Some(value) = clean_optional_text(self.explanation.as_deref()) {
            if plan.explanation != value {
                plan.explanation = value;
                changed = true;
            }
        }

        changed
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoachChatResponse {
    #[serde(default)]
    pub reply: String,
    /// The model must echo the authoritative current round. Rust validates this
    /// before accepting the reply so a stale/misread round cannot silently leak
    /// into user-facing coaching.
    #[serde(default)]
    pub observed_round: u32,
    /// Card facts mentioned in the reply should cite their CardIds here. Rust
    /// verifies every cited id is present in the trusted HDT/snapshot context.
    #[serde(default)]
    pub cited_card_ids: Vec<String>,
    /// `null` for ordinary discussion. Present only when the user clearly asks
    /// to correct/change the current strategic goal or preferences.
    #[serde(default)]
    pub plan_patch: Option<RoundPlanPatch>,
    #[serde(default)]
    pub patch_summary: String,
}

fn clean_optional_text(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn clean_text_list(values: &[String], limit: usize) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .take(limit)
        .map(ToOwned::to_owned)
        .collect()
}

fn patch_weight(target: &mut f32, incoming: Option<f32>) -> bool {
    let Some(value) = incoming else { return false; };
    let value = if value.is_finite() { value.clamp(0.0, 3.0) } else { return false; };
    if (*target - value).abs() <= f32::EPSILON {
        false
    } else {
        *target = value;
        true
    }
}

// -----------------------------------------------------------------------------
// V0.4 dynamic decision models
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionWeights {
    #[serde(default = "one")]
    pub tempo: f32,
    #[serde(default = "one")]
    pub scaling: f32,
    #[serde(default = "one")]
    pub economy: f32,
    #[serde(default = "one")]
    pub synergy: f32,
    #[serde(default = "one")]
    pub triple: f32,
    #[serde(default = "one")]
    pub survival: f32,
}

impl Default for DecisionWeights {
    fn default() -> Self {
        Self { tempo: 1.0, scaling: 1.0, economy: 1.0, synergy: 1.0, triple: 1.0, survival: 1.0 }
    }
}

impl DecisionWeights {
    pub fn normalized(&self) -> Self {
        Self {
            tempo: finite_clamp(self.tempo),
            scaling: finite_clamp(self.scaling),
            economy: finite_clamp(self.economy),
            synergy: finite_clamp(self.synergy),
            triple: finite_clamp(self.triple),
            survival: finite_clamp(self.survival),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomyPolicy {
    #[serde(default = "default_rerolls")]
    pub max_rerolls: u8,
    #[serde(default)]
    pub gold_reserve: i32,
    #[serde(default)]
    pub reason: String,
}

impl Default for EconomyPolicy {
    fn default() -> Self {
        Self { max_rerolls: default_rerolls(), gold_reserve: 0, reason: String::new() }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpgradePosture {
    #[serde(alias = "Prefer", alias = "PREFER")]
    Prefer,
    #[serde(alias = "Neutral", alias = "NEUTRAL")]
    Neutral,
    #[serde(alias = "Delay", alias = "DELAY")]
    Delay,
}

impl Default for UpgradePosture {
    fn default() -> Self {
        Self::Neutral
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierPolicy {
    /// Strategic preference only. The deterministic local tier-curve can still
    /// override a soft Delay when the player has fallen badly behind curve.
    #[serde(default)]
    pub posture: UpgradePosture,
    #[serde(default)]
    pub target_tier: Option<u8>,
    #[serde(default)]
    pub reason: String,
}

impl Default for TierPolicy {
    fn default() -> Self {
        Self {
            posture: UpgradePosture::Neutral,
            target_tier: None,
            reason: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoundPlan {
    #[serde(default)]
    pub target_round: u32,
    #[serde(default)]
    pub primary_goal: String,
    #[serde(default)]
    pub secondary_goals: Vec<String>,
    #[serde(default)]
    pub direction: Vec<String>,
    #[serde(default)]
    pub economy: EconomyPolicy,
    #[serde(default)]
    pub tier_policy: TierPolicy,
    #[serde(default)]
    pub weights: DecisionWeights,
    #[serde(default)]
    pub replan_conditions: Vec<String>,
    #[serde(default)]
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrinketRolePriority {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub weight: f32,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrinketPlan {
    #[serde(default)]
    pub target_round: u32,
    #[serde(default)]
    pub desired_effect: String,
    #[serde(default)]
    pub role_priorities: Vec<TrinketRolePriority>,
    #[serde(default)]
    pub avoid_roles: Vec<String>,
    #[serde(default)]
    pub flexibility_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ChoiceOptionView {
    #[serde(default)]
    pub card_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrinketOptionRecommendation {
    #[serde(default)]
    pub card_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub score: f32,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrinketRankingResponse {
    #[serde(default)]
    pub recommendations: Vec<TrinketOptionRecommendation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StrategicPlanResponse {
    #[serde(default)]
    pub round_plan: RoundPlan,
    #[serde(default)]
    pub trinket_plan: Option<TrinketPlan>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ReplanLevel {
    Tactical,
    Strategic,
    LongTerm,
}

impl Default for ReplanLevel {
    fn default() -> Self { Self::Tactical }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReplanEvent {
    pub level: ReplanLevel,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub detail: String,
}

impl ReplanEvent {
    pub fn new(level: ReplanLevel, code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { level, code: code.into(), detail: detail.into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActionRecommendation {
    pub kind: String,
    pub score: f32,
    pub label: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub card_id: Option<String>,
    #[serde(default)]
    pub shop_index: Option<usize>,
    #[serde(default)]
    pub gold_after: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TacticalPlan {
    pub round_number: u32,
    pub shop_revision: u32,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub route: Vec<ActionRecommendation>,
    #[serde(default)]
    pub ranked_actions: Vec<ActionRecommendation>,
}

fn one() -> f32 { 1.0 }
fn default_rerolls() -> u8 { 2 }
fn finite_clamp(value: f32) -> f32 { if value.is_finite() { value.clamp(0.0, 3.0) } else { 1.0 } }
