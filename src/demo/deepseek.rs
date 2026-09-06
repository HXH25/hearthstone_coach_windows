use std::{
    collections::BTreeSet,
    error::Error,
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};

use super::{
    agent::AgentSnapshot,
    compliance::now_ms,
    config::DeepSeekConfig,
    hdt_knowledge::HdtKnowledgeBase,
    model::{
        AiUsage, ChatMessage, ChoiceOptionView, CoachChatResponse, CompositionOption,
        CompositionResponse, ReplanEvent, RoundPlan, StrategicPlanResponse, TacticalPlan,
        TrinketOptionRecommendation, TrinketPlan, TrinketRankingResponse, WatchlistResponse,
    },
};

#[derive(Debug)]
pub enum DeepSeekError {
    MissingApiKey,
    Http(String),
    EmptyContent,
    Json(serde_json::Error),
    InvalidResponse(String),
    Cancelled,
    Runtime(String),
}

impl fmt::Display for DeepSeekError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingApiKey => write!(f, "DeepSeek API key is empty; configure hearthcoach_demo.json first"),
            Self::Http(message) => write!(f, "DeepSeek HTTP error: {message}"),
            Self::EmptyContent => write!(f, "DeepSeek returned empty content"),
            Self::Json(error) => write!(f, "DeepSeek JSON parse error: {error}"),
            Self::InvalidResponse(message) => write!(f, "DeepSeek response error: {message}"),
            Self::Cancelled => write!(f, "AI task cancelled"),
            Self::Runtime(message) => write!(f, "AI runtime error: {message}"),
        }
    }
}

impl Error for DeepSeekError {}

impl From<serde_json::Error> for DeepSeekError {
    fn from(value: serde_json::Error) -> Self { Self::Json(value) }
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
    #[serde(default)]
    error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
struct Choice { message: Message }
#[derive(Debug, Deserialize)]
struct Message { content: Option<String> }
#[derive(Debug, Deserialize)]
struct ApiError { message: String }
#[derive(Debug, Deserialize)]
struct Usage {
    #[serde(default)] prompt_tokens: u64,
    #[serde(default)] completion_tokens: u64,
    #[serde(default)] total_tokens: u64,
    // DeepSeek exposes cache usage as top-level fields.
    #[serde(default)] prompt_cache_hit_tokens: u64,
    #[serde(default)] prompt_cache_miss_tokens: u64,
    // OpenAI-compatible providers commonly expose cached input here instead.
    #[serde(default)] prompt_tokens_details: Option<PromptTokenDetails>,
}

#[derive(Debug, Deserialize, Default)]
struct PromptTokenDetails {
    #[serde(default)] cached_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct ApiCallObservation {
    pub task_id: Option<u64>,
    pub kind: String,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
    pub status: String,
    pub error: Option<String>,
    pub usage: AiUsage,
}

pub type ApiCallObserver = Arc<dyn Fn(ApiCallObservation) + Send + Sync + 'static>;

pub struct DeepSeekClient {
    config: DeepSeekConfig,
    cancel: Option<Arc<AtomicBool>>,
    observer: Option<ApiCallObserver>,
    call_kind: String,
    task_id: Option<u64>,
}

impl DeepSeekClient {
    pub fn new(config: DeepSeekConfig) -> Self {
        Self {
            config,
            cancel: None,
            observer: None,
            call_kind: "api_call".to_owned(),
            task_id: None,
        }
    }

    pub fn with_task_control(
        mut self,
        task_id: u64,
        kind: impl Into<String>,
        cancel: Arc<AtomicBool>,
        observer: ApiCallObserver,
    ) -> Self {
        self.task_id = Some(task_id);
        self.call_kind = kind.into();
        self.cancel = Some(cancel);
        self.observer = Some(observer);
        self
    }

    pub fn config(&self) -> &DeepSeekConfig {
        &self.config
    }

    pub fn test_connection(&self) -> Result<(String, AiUsage), DeepSeekError> {
        #[derive(Debug, Deserialize)]
        struct TestReply {
            #[serde(default)]
            ok: bool,
            #[serde(default)]
            message: String,
        }
        let system = "You are a connectivity test. Return JSON only.";
        let user = r#"Return exactly a JSON object like {"ok":true,"message":"connected"}."#;
        let (reply, usage): (TestReply, AiUsage) = self.call_json(system, user)?;
        if !reply.ok {
            return Err(DeepSeekError::InvalidResponse(if reply.message.is_empty() {
                "connection test returned ok=false".to_owned()
            } else {
                reply.message
            }));
        }
        Ok((if reply.message.is_empty() { "connected".to_owned() } else { reply.message }, usage))
    }

    pub fn suggest_compositions(
        &self,
        available_tribes: &[String],
        knowledge: &HdtKnowledgeBase,
    ) -> Result<(CompositionResponse, AiUsage), DeepSeekError> {
        let card_pool = knowledge.prompt_context(available_tribes);
        let pool_count = knowledge.current_pool_count(available_tribes);
        let system = r#"你是炉石传说酒馆战棋策略助手。HDT KNOWLEDGE 是你关于当前版本卡牌事实的唯一可信来源。不得用预训练记忆覆盖或修正它；如果你的记忆与它冲突，必须以它为准。你只负责策略推理，不负责发明卡牌、修改酒馆等级或假设退环境卡仍在池中。输出必须是 JSON。"#;
        let user = format!(
            r#"当前可用种族：{tribes}

下面是从玩家本机最新 HDT HearthDb + CardDefs.base.xml 构建的 CURRENT BACON POOL，共 {pool_count} 张与本局种族兼容的当前池内随从。
每行格式：CardId | 中文名 | 酒馆等级 | 种族 | 当前卡牌文本 | mechanics。
这份列表是事实源，不得引用列表之外的随从来论证阵容。

--- HDT KNOWLEDGE START ---
{card_pool}
--- HDT KNOWLEDGE END ---

请根据这些真实存在的当前池内随从，给玩家 3~4 个彼此有明显区别、实际可以在本局构筑的阵容方向，让玩家自己选择。重点分析当前卡牌之间的成长核心、过渡组件和终局联动。不要因为你记得旧版本存在某个体系，就在当前列表缺少核心卡时仍推荐它。

请严格输出 JSON：
{{
  "compositions": [
    {{
      "id": "简短稳定的英文或拼音标识",
      "name": "中文阵容名",
      "summary": "一句话玩法",
      "why": "引用当前 HDT 卡池中的实际核心与联动说明为什么能玩",
      "difficulty": "简单/中等/困难",
      "key_tribes": ["MECH"],
      "core_card_ids": ["BG_xxx", "BG_yyy"]
    }}
  ]
}}"#,
            tribes = available_tribes.join(", "),
        );
        let (mut response, usage): (CompositionResponse, AiUsage) = self.call_json(system, &user)?;
        response.compositions.retain_mut(|composition| {
            let mut seen = std::collections::BTreeSet::new();
            composition.core_card_ids = composition
                .core_card_ids
                .iter()
                .filter_map(|card_id| knowledge.validate_pool_card_id(card_id, available_tribes))
                .filter(|card_id| seen.insert(card_id.clone()))
                .collect();
            composition.core_card_ids.len() >= 2
        });
        if response.compositions.len() < 2 {
            return Err(DeepSeekError::InvalidResponse(
                "fewer than two composition suggestions survived current HDT pool validation".to_owned(),
            ));
        }
        Ok((response, usage))
    }

    pub fn build_watchlist(
        &self,
        available_tribes: &[String],
        selected: &CompositionOption,
        knowledge: &HdtKnowledgeBase,
    ) -> Result<(WatchlistResponse, AiUsage), DeepSeekError> {
        let card_pool = knowledge.prompt_context(available_tribes);
        let selected_json = serde_json::to_string(selected).unwrap_or_default();
        let system = r#"你是炉石传说酒馆战棋策略助手。HDT KNOWLEDGE 是当前版本卡牌事实的唯一来源。你只能从用户提供的 CURRENT BACON POOL 中选择 CardId。不得输出列表外 CardId，不得凭记忆补充旧卡。你只负责给当前真实卡牌排序并解释作用。输出必须是 JSON。"#;
        let user = format!(
            r#"当前可用种族：{tribes}
玩家已经选择的阵容：{selected_json}

下面是玩家本机最新 HDT 当前酒馆随从池。CardId、中文名、酒馆等级、种族和卡牌文字都由 Rust/HDT 决定，你不要自行改写这些事实。

--- HDT KNOWLEDGE START ---
{card_pool}
--- HDT KNOWLEDGE END ---

请把选定阵容转换成前期、中期、后期三段“商店随从观察列表”。每个阶段建议 5~12 张真正值得特别提醒的当前池内随从。只返回 CardId + 策略判断：
- S：核心、看见通常应该强烈考虑；
- A：强力组件或重要过渡；
- B：情境组件/补强。
role 写短标签，例如“核心成长”“过渡战力”“找三连”“功能牌”“经济”。reason 用 1~2 句中文说明为什么当前阵容需要它。
同一卡可以出现在多个阶段，但没有必要就不要重复。

严格输出 JSON：
{{
  "composition_id": "{comp_id}",
  "stages": [
    {{
      "stage": "early",
      "cards": [{{"card_id":"BG_xxx","priority":"S","role":"核心","reason":"..."}}]
    }},
    {{"stage": "mid", "cards": []}},
    {{"stage": "late", "cards": []}}
  ]
}}"#,
            tribes = available_tribes.join(", "),
            comp_id = selected.id,
        );
        let (mut response, usage): (WatchlistResponse, AiUsage) = self.call_json(system, &user)?;
        let catalog = knowledge.catalog();
        for stage in &mut response.stages {
            let mut seen = std::collections::BTreeSet::new();
            stage.cards.retain_mut(|card| {
                let Some(canonical) = knowledge.validate_pool_card_id(&card.card_id, available_tribes) else {
                    return false;
                };
                if !seen.insert(canonical.clone()) {
                    return false;
                }
                card.card_id = canonical;
                card.priority = normalize_priority(&card.priority);
                if let Some(meta) = catalog.resolve(&card.card_id) {
                    card.name = meta.preferred_name().unwrap_or(&card.card_id).to_owned();
                    card.tavern_tier = meta.tavern_tier();
                }
                true
            });
        }
        let missing_stage = ["early", "mid", "late"].into_iter().find(|stage_name| {
            response
                .stages
                .iter()
                .find(|stage| stage.stage.as_str() == *stage_name)
                .map(|stage| stage.cards.is_empty())
                .unwrap_or(true)
        });
        if let Some(stage) = missing_stage {
            return Err(DeepSeekError::InvalidResponse(format!(
                "HDT validation left the {stage} watchlist empty; regenerate instead of silently using stale/invalid cards"
            )));
        }
        Ok((response, usage))
    }

    pub fn plan_next_recruit(
        &self,
        snapshot: &AgentSnapshot,
        selected: Option<&CompositionOption>,
        watchlist: Option<&WatchlistResponse>,
        previous_plan: Option<&RoundPlan>,
        replan_events: &[ReplanEvent],
        target_round: u32,
        need_trinket_plan: bool,
    ) -> Result<(StrategicPlanResponse, AiUsage), DeepSeekError> {
        let snapshot_json = serde_json::to_string(snapshot).unwrap_or_default();
        let selected_json = serde_json::to_string(&selected).unwrap_or_else(|_| "null".to_owned());
        let watchlist_json = serde_json::to_string(&watchlist).unwrap_or_else(|_| "null".to_owned());
        let previous_json = serde_json::to_string(&previous_plan).unwrap_or_else(|_| "null".to_owned());
        let replan_json = serde_json::to_string(replan_events).unwrap_or_else(|_| "[]".to_owned());
        let system = r#"你是炉石传说酒馆战棋的慢速战略规划器。你只负责“下一 Recruit 回合想完成什么”，不直接替玩家逐点击操作。Rust 本地 Tactical/Action 层会根据你的结构化小目标实时执行。你必须保持计划稳定：没有提供重大 ReplanEvent 时，不要因为商店小变化反复改方向。不得编造当前状态中不存在的卡牌事实。输出必须是 JSON。"#;
        let user = format!(
            r#"目标：为第 {target_round} 回合 Recruit 生成一个可执行的小目标。

当前 Harness 稳定状态：
{snapshot_json}

玩家选定阵容方向（可能为空）：
{selected_json}

已有 HDT Watchlist（可能为空，仅作当前卡牌优先级参考）：
{watchlist_json}

上一版 RoundPlan（可能为空）：
{previous_json}

触发本次重规划的重大事件：
{replan_json}

规则：
1. primary_goal 必须是一个这一回合能执行/验证的小目标，例如“补两张即时战力并延后升本”，不要只写“变强”。
2. weights 每项范围 0~3：tempo/scaling/economy/synergy/triple/survival。低血量应提高 tempo/survival；高血量允许更多 scaling/economy。
3. economy.max_rerolls 给 0~8 的刷新预算，gold_reserve 是计划保留金币。
4. tier_policy.posture 只能是 prefer / neutral / delay。它只是战略偏好，不是硬禁令；Rust 会同时使用回合等级曲线、血量和商店机会成本决定是否升本。随着回合推进，低本过渡牌会在本地自动贬值，因此不要因为早期 S 牌而长期延后升本。
5. replan_conditions 只列真正会破坏当前战略的条件。
6. need_trinket_plan={need_trinket_plan}。若为 true，额外规划“希望饰品解决什么问题”的作用优先级，不要猜具体会出现哪件饰品；role_priorities 的 weight 范围 0~10。若为 false，trinket_plan=null。
7. 所有说明使用简洁中文。

严格输出：
{{
  "round_plan": {{
    "target_round": {target_round},
    "primary_goal": "...",
    "secondary_goals": ["..."],
    "direction": ["MECH"],
    "economy": {{"max_rerolls": 3, "gold_reserve": 0, "reason": "..."}},
    "tier_policy": {{"posture": "neutral", "target_tier": 3, "reason": "..."}},
    "weights": {{"tempo":1.5,"scaling":1.0,"economy":0.8,"synergy":1.2,"triple":1.0,"survival":1.4}},
    "replan_conditions": ["获得体系核心", "生命跌入危险线"],
    "explanation": "..."
  }},
  "trinket_plan": null
}}
"#,
        );
        let (mut response, usage): (StrategicPlanResponse, AiUsage) = self.call_json(system, &user)?;
        response.round_plan.target_round = target_round;
        response.round_plan.economy.max_rerolls = response.round_plan.economy.max_rerolls.min(8);
        response.round_plan.economy.gold_reserve = response.round_plan.economy.gold_reserve.max(0);
        response.round_plan.weights = response.round_plan.weights.normalized();
        response.round_plan.direction.retain(|item| !item.trim().is_empty());
        response.round_plan.direction.truncate(4);
        response.round_plan.secondary_goals.truncate(4);
        response.round_plan.replan_conditions.truncate(6);
        if need_trinket_plan {
            let trinket = response.trinket_plan.get_or_insert_with(Default::default);
            trinket.target_round = target_round;
            for role in &mut trinket.role_priorities {
                role.weight = if role.weight.is_finite() { role.weight.clamp(0.0, 10.0) } else { 0.0 };
            }
            trinket.role_priorities.sort_by(|a, b| b.weight.total_cmp(&a.weight));
            trinket.role_priorities.truncate(6);
            trinket.avoid_roles.truncate(5);
        } else {
            response.trinket_plan = None;
        }
        if response.round_plan.primary_goal.trim().is_empty() {
            return Err(DeepSeekError::InvalidResponse("round_plan.primary_goal is empty".to_owned()));
        }
        Ok((response, usage))
    }

    pub fn rank_trinket_options(
        &self,
        options: &[ChoiceOptionView],
        round_plan: Option<&RoundPlan>,
        trinket_plan: Option<&TrinketPlan>,
    ) -> Result<(TrinketRankingResponse, AiUsage), DeepSeekError> {
        if options.is_empty() {
            return Err(DeepSeekError::InvalidResponse("no trinket options".to_owned()));
        }
        let options_json = serde_json::to_string(options).unwrap_or_else(|_| "[]".to_owned());
        let round_json = serde_json::to_string(&round_plan).unwrap_or_else(|_| "null".to_owned());
        let trinket_json = serde_json::to_string(&trinket_plan).unwrap_or_else(|_| "null".to_owned());
        let system = r#"你是炉石传说酒馆战棋的饰品选择器。候选饰品的 CardId、名字和文字由 Harness/HearthDb 提供，是唯一事实源。你只能在这些候选中排序，不能发明候选。结合当前 RoundPlan 与提前生成的 TrinketPlan，给每个候选 0~10 分，并说明它具体解决什么问题。输出必须是 JSON。"#;
        let user = format!(
            r#"当前实际出现的饰品候选：
{options_json}

当前 RoundPlan：
{round_json}

提前规划的 TrinketPlan：
{trinket_json}

请对所有实际候选饰品排序。card_id 必须原样复制候选中的值。name 可填写，但 Rust 会用事实源覆盖。role 写短标签，例如“即时战力/体系成长/经济/保护”。reason 用一句简洁中文说明为什么它适合或不适合当前计划。

严格输出：
{{
  "recommendations": [
    {{"card_id":"实际候选CardId","name":"实际饰品名","score":9.2,"role":"体系成长","reason":"..."}}
  ]
}}"#,
        );
        let (mut response, usage): (TrinketRankingResponse, AiUsage) =
            self.call_json(system, &user)?;

        let by_id = options
            .iter()
            .map(|option| (option.card_id.as_str(), option))
            .collect::<std::collections::HashMap<_, _>>();
        let mut seen = std::collections::BTreeSet::new();
        response.recommendations.retain_mut(|item| {
            let Some(option) = by_id.get(item.card_id.as_str()) else {
                return false;
            };
            if !seen.insert(item.card_id.clone()) {
                return false;
            }
            item.name = option.name.clone();
            item.score = if item.score.is_finite() {
                item.score.clamp(0.0, 10.0)
            } else {
                0.0
            };
            item.role = item.role.trim().to_owned();
            item.reason = item.reason.trim().to_owned();
            true
        });

        for option in options {
            if seen.insert(option.card_id.clone()) {
                response.recommendations.push(TrinketOptionRecommendation {
                    card_id: option.card_id.clone(),
                    name: option.name.clone(),
                    score: 0.0,
                    role: "未排序".to_owned(),
                    reason: "模型未返回该候选；保留在列表中供玩家比较".to_owned(),
                });
            }
        }
        response
            .recommendations
            .sort_by(|a, b| b.score.total_cmp(&a.score));
        response.recommendations.truncate(options.len());
        Ok((response, usage))
    }

    pub fn chat_with_coach(
        &self,
        message: &str,
        snapshot: &AgentSnapshot,
        selected: Option<&CompositionOption>,
        watchlist: Option<&WatchlistResponse>,
        round_plan: Option<&RoundPlan>,
        tactical_plan: Option<&TacticalPlan>,
        trinket_plan: Option<&TrinketPlan>,
        history: &[ChatMessage],
        knowledge: &HdtKnowledgeBase,
    ) -> Result<(CoachChatResponse, AiUsage), DeepSeekError> {
        if message.trim().is_empty() {
            return Err(DeepSeekError::InvalidResponse("empty chat message".to_owned()));
        }

        let (card_facts, trusted_card_ids) = chat_card_facts(
            knowledge,
            snapshot,
            selected,
            watchlist,
        );
        let selected_core_card_ids = selected
            .map(|item| item.core_card_ids.iter().cloned().collect::<BTreeSet<_>>())
            .unwrap_or_default();
        let snapshot_json = serde_json::to_string(snapshot).unwrap_or_default();
        let selected_json = serde_json::to_string(&selected).unwrap_or_else(|_| "null".to_owned());
        let watchlist_json = serde_json::to_string(&watchlist).unwrap_or_else(|_| "null".to_owned());
        let plan_json = serde_json::to_string(&round_plan).unwrap_or_else(|_| "null".to_owned());
        let tactical_json = serde_json::to_string(&tactical_plan).unwrap_or_else(|_| "null".to_owned());
        let trinket_json = serde_json::to_string(&trinket_plan).unwrap_or_else(|_| "null".to_owned());
        let history_json = serde_json::to_string(history).unwrap_or_else(|_| "[]".to_owned());
        let stage = stage_for_round(snapshot.round_number);
        let selected_core_ids = selected
            .map(|item| item.core_card_ids.join(","))
            .unwrap_or_default();
        let current_stage_watch_ids = watchlist
            .and_then(|list| list.stages.iter().find(|item| item.stage == stage))
            .map(|item| item.cards.iter().map(|card| card.card_id.as_str()).collect::<Vec<_>>().join(","))
            .unwrap_or_default();
        let current_facts = format!(
            "CURRENT_ROUND={}\nCURRENT_PHASE={}\nCURRENT_STAGE={}\nHP={:?}\nGOLD={:?}\nTAVERN_TIER={:?}\nREFRESH_COST={:?}\nUPGRADE_COST={:?}\nAVAILABLE_TRIBES={}\nSELECTED_CORE_CARD_IDS={}\nCURRENT_STAGE_WATCHLIST_IDS={}",
            snapshot.round_number,
            snapshot.phase,
            stage,
            snapshot.hero_effective_health,
            snapshot.gold,
            snapshot.tavern_tier,
            snapshot.refresh_cost,
            snapshot.upgrade_cost,
            snapshot.available_tribes.join(","),
            selected_core_ids,
            current_stage_watch_ids,
        );

        let system = r#"你是 HearthCoach 的对话教练。你可以和玩家讨论酒馆战棋思路，也可以在玩家明确要求时修正当前 RoundPlan。

事实优先级（从高到低）：
1. CURRENT FACTS：Rust/Harness 当前实时状态，尤其 CURRENT_ROUND/CURRENT_PHASE/TAVERN_TIER，绝对不能被历史对话或模型记忆覆盖。
2. TRUSTED CARD FACTS：来自本机 HDT/HearthDb 的 CardId/名称/等级/文本，是具体卡牌事实的唯一来源。
3. CURRENT TACTICAL PLAN：Rust 当前动作排序。解释“为什么现在升本/买牌/刷新”时必须以这里的当前排序为准，不得自行声称系统推荐了不存在的动作。
4. RoundPlan / Selected Composition / Watchlist：战略计划与推荐标签，不是卡牌事实源。
5. 最近对话：仅提供上下文，若与上述最新状态冲突必须忽略旧信息。

重要规则：
1. 普通提问、讨论、质疑、让你解释理由时，只回复，不修改计划，plan_patch 必须为 null。
2. 玩家明确表达“改成/不要/继续/优先/延后/我想升本/我决定转向”等策略意图时，才可以返回 plan_patch，只包含需要修改的字段。
3. 不得修改 target_round；这是 Rust 的权威字段。
4. plan_patch 的 weights 范围 0~3，max_rerolls 0~8，target_tier 1~6，upgrade posture 只能 prefer/neutral/delay。
5. 讨论具体卡牌时，只能使用 TRUSTED CARD FACTS 中存在的卡；若信息不足，明确说“当前事实上下文不足”，不要凭预训练记忆补卡。
6. 若玩家问“当前阵容的核心卡”，优先依据 selected composition 的 core_card_ids，并结合当前阶段 Watchlist 解释，不要另造一套核心卡。
7. 每当回复提到具体卡牌事实，必须把对应 CardId 放进 cited_card_ids；不得引用未提供的 CardId。
8. observed_round 必须等于 CURRENT_ROUND。除非玩家明确问其他回合，否则 reply 中不得把当前局面说成别的回合。
9. 玩家最终意图优先于原计划；如果风险很大，在 reply 中说明风险，同时仍可按明确要求给 patch。
10. 输出必须是 JSON，中文简洁但完整。"#;

        let user = format!(
            r#"玩家刚刚说：
{message}

--- CURRENT FACTS (AUTHORITATIVE) ---
{current_facts}

当前 Harness snapshot：
{snapshot_json}

当前 Rust Tactical Plan：
{tactical_json}

玩家选择的阵容：
{selected_json}

当前 Watchlist：
{watchlist_json}

--- TRUSTED CARD FACTS (HDT/HearthDb) ---
{card_facts}
--- END TRUSTED CARD FACTS ---

当前 RoundPlan：
{plan_json}

当前 TrinketPlan：
{trinket_json}

最近对话：
{history_json}

严格输出：
{{
  "reply": "给玩家的自然语言回答",
  "observed_round": {round},
  "cited_card_ids": [],
  "plan_patch": null,
  "patch_summary": ""
}}

如果玩家明确要求修改目标，则 plan_patch 可使用这些可选字段：
{{
  "primary_goal": "...",
  "secondary_goals": ["..."],
  "direction": ["ELEMENTAL"],
  "economy": {{"max_rerolls": 2, "gold_reserve": 0, "reason": "..."}},
  "tier_policy": {{"posture": "prefer", "target_tier": 5, "reason": "..."}},
  "weights": {{"tempo": 1.0, "scaling": 1.8, "economy": 1.1, "synergy": 1.6, "triple": 1.0, "survival": 0.8}},
  "replan_conditions": ["..."],
  "explanation": "..."
}}
没有修改意图就不要生成 patch。"#,
            message = message.trim(),
            round = snapshot.round_number,
        );

        let (mut response, mut usage): (CoachChatResponse, AiUsage) = self.call_json(system, &user)?;
        let allowed_plan_round = round_plan.map(|plan| plan.target_round).filter(|round| *round > 0);
        let first_error = validate_chat_grounding(
            &response,
            message,
            snapshot.round_number,
            allowed_plan_round,
            &trusted_card_ids,
            &selected_core_card_ids,
        )
        .err();

        if let Some(reason) = first_error {
            let previous = serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_owned());
            let repair = format!(
                "{user}\n\n上一份回答未通过 Rust grounding 校验：{reason}\n上一份回答：{previous}\n请重新生成。必须严格使用 CURRENT_ROUND={}，具体卡牌只能来自 TRUSTED CARD FACTS，并正确填写 cited_card_ids。",
                snapshot.round_number,
            );
            let (fixed, retry_usage): (CoachChatResponse, AiUsage) = self.call_json(system, &repair)?;
            usage.prompt_tokens = usage.prompt_tokens.saturating_add(retry_usage.prompt_tokens);
            usage.completion_tokens = usage.completion_tokens.saturating_add(retry_usage.completion_tokens);
            usage.total_tokens = usage.total_tokens.saturating_add(retry_usage.total_tokens);
            usage.prompt_cache_hit_tokens = usage
                .prompt_cache_hit_tokens
                .saturating_add(retry_usage.prompt_cache_hit_tokens);
            usage.prompt_cache_miss_tokens = usage
                .prompt_cache_miss_tokens
                .saturating_add(retry_usage.prompt_cache_miss_tokens);
            validate_chat_grounding(
                &fixed,
                message,
                snapshot.round_number,
                allowed_plan_round,
                &trusted_card_ids,
                &selected_core_card_ids,
            )?;
            response = fixed;
        }

        response.reply = response.reply.trim().to_owned();
        if response.reply.is_empty() {
            return Err(DeepSeekError::InvalidResponse("chat reply is empty".to_owned()));
        }
        if response.plan_patch.as_ref().map(|patch| patch.is_empty()).unwrap_or(false) {
            response.plan_patch = None;
        }
        response.patch_summary = response.patch_summary.trim().to_owned();
        response.cited_card_ids.sort();
        response.cited_card_ids.dedup();
        Ok((response, usage))
    }

    fn call_json<T: DeserializeOwned>(&self, system: &str, user: &str) -> Result<(T, AiUsage), DeepSeekError> {
        if self.config.api_key.trim().is_empty()
            && self.config.api_compatibility.eq_ignore_ascii_case("deepseek")
        {
            return Err(DeepSeekError::MissingApiKey);
        }
        if self.is_cancelled() {
            return Err(DeepSeekError::Cancelled);
        }

        let started_at_ms = now_ms();
        let url = format!("{}/chat/completions", self.config.base_url.trim_end_matches('/'));
        let (system, user) = fit_prompt_to_context(
            system,
            user,
            self.config.context_window,
            self.config.max_tokens,
        );
        let mut body = serde_json::Map::new();
        body.insert("model".to_owned(), json!(self.config.model));
        body.insert(
            "messages".to_owned(),
            json!([
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ]),
        );
        body.insert("max_tokens".to_owned(), json!(self.config.max_tokens));
        body.insert("temperature".to_owned(), json!(0.3));
        if self.config.api_compatibility.eq_ignore_ascii_case("deepseek") {
            body.insert(
                "thinking".to_owned(),
                json!({"type": if self.config.thinking { "enabled" } else { "disabled" }}),
            );
            body.insert("response_format".to_owned(), json!({"type":"json_object"}));
        }

        let value = match self.execute_http_json(&url, Value::Object(body)) {
            Ok(value) => value,
            Err(error) => {
                let status = if matches!(&error, DeepSeekError::Cancelled) {
                    "cancelled"
                } else {
                    "failed"
                };
                self.observe_call(
                    started_at_ms,
                    status,
                    Some(error.to_string()),
                    AiUsage::default(),
                );
                return Err(error);
            }
        };
        let response: ChatResponse = match serde_json::from_value(value) {
            Ok(response) => response,
            Err(error) => {
                self.observe_call(
                    started_at_ms,
                    "failed",
                    Some(error.to_string()),
                    AiUsage::default(),
                );
                return Err(DeepSeekError::Json(error));
            }
        };
        let usage = response
            .usage
            .map(|usage| {
                let nested_cached = usage
                    .prompt_tokens_details
                    .as_ref()
                    .map(|details| details.cached_tokens)
                    .unwrap_or(0);
                let cache_hit = usage.prompt_cache_hit_tokens.max(nested_cached);
                let cache_miss = if usage.prompt_cache_miss_tokens > 0 {
                    usage.prompt_cache_miss_tokens
                } else {
                    usage.prompt_tokens.saturating_sub(cache_hit)
                };
                let total_tokens = if usage.total_tokens > 0 {
                    usage.total_tokens
                } else {
                    usage.prompt_tokens.saturating_add(usage.completion_tokens)
                };
                AiUsage {
                    prompt_tokens: usage.prompt_tokens,
                    completion_tokens: usage.completion_tokens,
                    total_tokens,
                    prompt_cache_hit_tokens: cache_hit,
                    prompt_cache_miss_tokens: cache_miss,
                }
            })
            .unwrap_or_default();
        if let Some(error) = response.error {
            self.observe_call(
                started_at_ms,
                "failed",
                Some(error.message.clone()),
                usage,
            );
            return Err(DeepSeekError::InvalidResponse(error.message));
        }
        let content = match response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
            .filter(|content| !content.trim().is_empty())
        {
            Some(content) => content,
            None => {
                self.observe_call(
                    started_at_ms,
                    "failed",
                    Some("empty content".to_owned()),
                    usage,
                );
                return Err(DeepSeekError::EmptyContent);
            }
        };
        let parsed = match parse_json_content::<T>(content) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.observe_call(
                    started_at_ms,
                    "invalid_json",
                    Some(error.to_string()),
                    usage,
                );
                return Err(DeepSeekError::Json(error));
            }
        };
        self.observe_call(started_at_ms, "ok", None, usage.clone());
        Ok((parsed, usage))
    }

    fn execute_http_json(&self, url: &str, body: Value) -> Result<Value, DeepSeekError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| DeepSeekError::Runtime(error.to_string()))?;
        let timeout = Duration::from_secs(self.config.timeout_seconds.clamp(3, 600));
        let api_key = self.config.api_key.trim().to_owned();
        let cancel = self.cancel.clone();
        runtime.block_on(async move {
            let client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(timeout)
                .build()
                .map_err(|error| DeepSeekError::Http(error.to_string()))?;
            let mut request = client.post(url).json(&body);
            if !api_key.is_empty() {
                request = request.bearer_auth(api_key);
            }
            let send = request.send();
            tokio::pin!(send);
            let response = loop {
                tokio::select! {
                    result = &mut send => {
                        break result.map_err(|error| DeepSeekError::Http(error.to_string()))?;
                    }
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        if cancel.as_ref().map(|flag| flag.load(Ordering::Acquire)).unwrap_or(false) {
                            return Err(DeepSeekError::Cancelled);
                        }
                    }
                }
            };
            let status = response.status();
            let read = response.text();
            tokio::pin!(read);
            let text = loop {
                tokio::select! {
                    result = &mut read => {
                        break result.map_err(|error| DeepSeekError::Http(error.to_string()))?;
                    }
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        if cancel.as_ref().map(|flag| flag.load(Ordering::Acquire)).unwrap_or(false) {
                            return Err(DeepSeekError::Cancelled);
                        }
                    }
                }
            };
            if !status.is_success() {
                return Err(DeepSeekError::Http(format!("HTTP {}: {}", status.as_u16(), text)));
            }
            serde_json::from_str::<Value>(&text).map_err(DeepSeekError::Json)
        })
    }

    fn is_cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .map(|flag| flag.load(Ordering::Acquire))
            .unwrap_or(false)
    }

    fn observe_call(
        &self,
        started_at_ms: u64,
        status: &str,
        error: Option<String>,
        usage: AiUsage,
    ) {
        if let Some(observer) = self.observer.as_ref() {
            observer(ApiCallObservation {
                task_id: self.task_id,
                kind: self.call_kind.clone(),
                started_at_ms,
                finished_at_ms: now_ms(),
                status: status.to_owned(),
                error,
                usage,
            });
        }
    }

}




fn parse_json_content<T: DeserializeOwned>(content: &str) -> Result<T, serde_json::Error> {
    let mut text = content.trim();
    let owned;
    if text.starts_with("```") {
        let mut lines = text.lines().collect::<Vec<_>>();
        if !lines.is_empty() {
            lines.remove(0);
        }
        if lines.last().map(|line| line.trim().starts_with("```")) == Some(true) {
            lines.pop();
        }
        owned = lines.join("\n");
        text = owned.trim();
    }
    match serde_json::from_str::<T>(text) {
        Ok(value) => Ok(value),
        Err(first_error) => {
            let Some(start) = text.find('{') else { return Err(first_error); };
            let Some(end) = text.rfind('}') else { return Err(first_error); };
            if end <= start {
                return Err(first_error);
            }
            serde_json::from_str::<T>(&text[start..=end])
        }
    }
}

fn fit_prompt_to_context(
    system: &str,
    user: &str,
    context_window: u32,
    max_output_tokens: u32,
) -> (String, String) {
    // Providers expose exact token usage only after the request. For pre-flight
    // safety we use a conservative character budget so the user-configured
    // context window actually constrains prompts without requiring a
    // provider-specific tokenizer.
    let input_tokens = context_window.saturating_sub(max_output_tokens).max(512) as usize;
    let max_chars = input_tokens.saturating_mul(2);
    let system_chars = system.chars().count();
    let user_chars = user.chars().count();
    if system_chars.saturating_add(user_chars) <= max_chars {
        return (system.to_owned(), user.to_owned());
    }
    let keep_system = system_chars.min(max_chars / 3);
    let keep_user = max_chars.saturating_sub(keep_system).max(256);
    let system_trimmed = system.chars().take(keep_system).collect::<String>();
    if user_chars <= keep_user {
        return (system_trimmed, user.to_owned());
    }
    let head = keep_user * 3 / 5;
    let tail = keep_user.saturating_sub(head);
    let mut user_trimmed = user.chars().take(head).collect::<String>();
    user_trimmed.push_str("\n\n...[context truncated by configured context_window]...\n\n");
    let tail_text = user
        .chars()
        .rev()
        .take(tail)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    user_trimmed.push_str(&tail_text);
    (system_trimmed, user_trimmed)
}

fn stage_for_round(round: u32) -> &'static str {
    match round {
        0..=4 => "early",
        5..=8 => "mid",
        _ => "late",
    }
}

fn chat_card_facts(
    knowledge: &HdtKnowledgeBase,
    snapshot: &AgentSnapshot,
    selected: Option<&CompositionOption>,
    watchlist: Option<&WatchlistResponse>,
) -> (String, BTreeSet<String>) {
    let mut ids = BTreeSet::<String>::new();
    for card in snapshot
        .board
        .iter()
        .chain(snapshot.hand.iter())
        .chain(snapshot.shop.iter())
        .chain(snapshot.trinkets.iter())
        .chain(snapshot.opponent_board.iter())
    {
        if !card.card_id.trim().is_empty() {
            ids.insert(card.card_id.clone());
        }
    }
    if let Some(selected) = selected {
        ids.extend(selected.core_card_ids.iter().filter(|id| !id.trim().is_empty()).cloned());
    }
    if let Some(watchlist) = watchlist {
        for stage in &watchlist.stages {
            ids.extend(stage.cards.iter().filter(|card| !card.card_id.trim().is_empty()).map(|card| card.card_id.clone()));
        }
    }

    let catalog = knowledge.catalog();
    let mut trusted = BTreeSet::new();
    let mut rows = Vec::new();
    for id in ids {
        let Some(meta) = catalog.resolve(&id) else { continue; };
        let canonical = meta.normal_card_id().unwrap_or(id.as_str()).to_owned();
        trusted.insert(id.clone());
        trusted.insert(canonical.clone());
        let name = meta.preferred_name().unwrap_or(&id);
        let text = meta.preferred_text().unwrap_or("").replace(['\r', '\n', '\t'], " ");
        rows.push(format!(
            "{} | {} | type={} | tier={:?} | tribes={} | text={}",
            canonical,
            name,
            meta.card_type().unwrap_or("UNKNOWN"),
            meta.tavern_tier(),
            meta.tribes().join("/"),
            text.split_whitespace().collect::<Vec<_>>().join(" "),
        ));
    }
    rows.sort();
    rows.dedup();
    (rows.join("\n"), trusted)
}

fn validate_chat_grounding(
    response: &CoachChatResponse,
    user_message: &str,
    current_round: u32,
    allowed_plan_round: Option<u32>,
    trusted_card_ids: &BTreeSet<String>,
    selected_core_card_ids: &BTreeSet<String>,
) -> Result<(), DeepSeekError> {
    if response.observed_round != current_round {
        return Err(DeepSeekError::InvalidResponse(format!(
            "chat observed_round={} but authoritative round is {}",
            response.observed_round, current_round
        )));
    }
    for card_id in &response.cited_card_ids {
        if !trusted_card_ids.contains(card_id) {
            return Err(DeepSeekError::InvalidResponse(format!(
                "chat cited untrusted card id {card_id}"
            )));
        }
    }
    let asks_card_facts = ["核心卡", "卡牌", "什么牌", "哪些牌", "哪张牌", "牌是什么"]
        .iter()
        .any(|needle| user_message.contains(needle));
    if asks_card_facts && response.cited_card_ids.is_empty() && !trusted_card_ids.is_empty() {
        return Err(DeepSeekError::InvalidResponse(
            "card-specific answer omitted cited_card_ids".to_owned(),
        ));
    }
    let asks_core_cards = user_message.contains("核心卡") || user_message.contains("核心牌");
    if asks_core_cards && !selected_core_card_ids.is_empty() {
        if response.cited_card_ids.is_empty()
            || !response
                .cited_card_ids
                .iter()
                .all(|id| selected_core_card_ids.contains(id))
        {
            return Err(DeepSeekError::InvalidResponse(
                "core-card answer must cite only selected composition core_card_ids".to_owned(),
            ));
        }
    }
    // Catch the most damaging stale-state failure observed in live testing:
    // claiming the current position is another numbered round. If the player
    // explicitly mentioned that round in their question, allow it as a
    // historical/hypothetical discussion.
    for round in 1..=30u32 {
        if round == current_round || allowed_plan_round == Some(round) {
            continue;
        }
        let phrase = format!("第{round}回合");
        let spaced = format!("第 {round} 回合");
        if (response.reply.contains(&phrase) || response.reply.contains(&spaced))
            && !user_message.contains(&phrase)
            && !user_message.contains(&spaced)
        {
            return Err(DeepSeekError::InvalidResponse(format!(
                "chat reply contradicts current round {current_round} by mentioning {phrase} as current context"
            )));
        }
    }
    Ok(())
}

fn normalize_priority(raw: &str) -> String {
    match raw.trim().to_ascii_uppercase().as_str() {
        "S" => "S".to_owned(),
        "A" => "A".to_owned(),
        _ => "B".to_owned(),
    }
}

#[cfg(test)]
mod chat_grounding_tests {
    use std::collections::BTreeSet;

    use super::validate_chat_grounding;
    use crate::demo::model::CoachChatResponse;

    #[test]
    fn rejects_stale_round_echo() {
        let response = CoachChatResponse {
            reply: "当前应该先稳住场面".to_owned(),
            observed_round: 1,
            ..Default::default()
        };
        assert!(validate_chat_grounding(&response, "现在要升本吗", 4, None, &BTreeSet::new(), &BTreeSet::new()).is_err());
    }

    #[test]
    fn rejects_untrusted_card_id() {
        let response = CoachChatResponse {
            reply: "这张牌是核心".to_owned(),
            observed_round: 4,
            cited_card_ids: vec!["BG_FAKE".to_owned()],
            ..Default::default()
        };
        assert!(validate_chat_grounding(&response, "核心牌是什么", 4, None, &BTreeSet::new(), &BTreeSet::new()).is_err());
    }

    #[test]
    fn accepts_current_round_and_trusted_card() {
        let response = CoachChatResponse {
            reply: "当前回合可以围绕这张核心牌继续构筑".to_owned(),
            observed_round: 4,
            cited_card_ids: vec!["BG_REAL".to_owned()],
            ..Default::default()
        };
        let trusted = ["BG_REAL".to_owned()].into_iter().collect();
        assert!(validate_chat_grounding(&response, "现在怎么走", 4, None, &trusted, &BTreeSet::new()).is_ok());
    }
}
