use std::{
    io::Read,
    path::Path,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use crate::harness::{
    CardCatalog, ChoiceKind, HarnessEvent, HarnessRuntime, PhaseKind, RecruitActionKind,
};

use super::{
    agent::{
        detect_replan, evaluate_recruit, fallback_strategic_plan, AgentCard, AgentSnapshot,
        DecisionFingerprint,
    },
    compliance::{
        calculate_cost, list_sessions, load_session, new_session_id, now_ms, save_session,
        summarize_usage, AgentSessionArchive, ApiCallRecord, ModelConfigSnapshot, SessionSummary,
        TaskHandle, TaskManager, TaskRecord, TraceEvent, UsageSummary,
    },
    config::{DemoConfig, DeepSeekConfig, OverlayConfig},
    deepseek::{ApiCallObservation, ApiCallObserver, DeepSeekClient, DeepSeekError},
    environment::{inspect_environment, prepare_environment, EnvironmentRepairResult, EnvironmentStatus},
    hdt_knowledge::{HdtKnowledgeBase, KnowledgeStats},
    model::{
        AiUsage, ChatMessage, ChoiceOptionView, CompositionOption, ReplanEvent, ReplanLevel,
        RoundPlan, ShopHit, TacticalPlan, TrinketOptionRecommendation, TrinketPlan,
        WatchlistResponse,
    },
};

const INDEX_HTML: &str = include_str!("../../web/index.html");

#[derive(Debug, Clone, Serialize, Default)]
pub struct PublicState {
    pub match_active: bool,
    pub current_round: u32,
    pub current_phase: String,
    pub phase_epoch: u64,
    pub shop_revision: u32,
    pub decision_revision: u32,
    pub overlay_marks_ready: bool,
    pub current_stage: String,
    pub available_tribes: Vec<String>,
    pub current_shop: Vec<ShopCardView>,
    /// Last stable Recruit board. It intentionally stays frozen throughout Combat.
    pub stable_board: Vec<AgentCard>,
    pub compositions: Vec<CompositionOption>,
    pub selected_composition: Option<CompositionOption>,
    pub watchlist: Option<WatchlistResponse>,
    pub shop_hits: Vec<ShopHit>,
    /// Dynamic immediate recommendations produced by the local Action Evaluator.
    pub decision_hits: Vec<ShopHit>,
    pub round_plan: Option<RoundPlan>,
    pub trinket_plan: Option<TrinketPlan>,
    pub tactical_plan: Option<TacticalPlan>,
    pub replan_events: Vec<ReplanEvent>,
    pub active_choice_kind: Option<ChoiceKind>,
    pub active_choice_option_count: usize,
    pub active_choice_options: Vec<ChoiceOptionView>,
    pub trinket_rankings: Vec<TrinketOptionRecommendation>,
    pub trinket_ranking_busy: bool,
    pub ai_status: String,
    pub error: Option<String>,
    pub api_key_configured: bool,
    pub model: String,
    pub base_url: String,
    pub usage: AiUsage,
    pub knowledge: KnowledgeStats,
    pub current_pool_count: usize,
    pub current_stage_recommendation_count: usize,
    pub overlay_test_mode: bool,
    /// In-match dialogue with the strategic coach. Only a short history is kept.
    pub chat_messages: Vec<ChatMessage>,
    pub chat_busy: bool,
    pub chat_status: String,
    /// Course-compliance state is also exposed to the debug Web UI.
    pub tasks: Vec<TaskRecord>,
    pub usage_summary: UsageSummary,
    pub session_id: Option<String>,
    pub trace_tail: Vec<TraceEvent>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ShopCardView {
    pub index: usize,
    pub card_id: String,
    pub name: String,
    pub tavern_tier: Option<u8>,
    pub text: String,
}

#[derive(Debug)]
struct MutableState {
    match_active: bool,
    current_round: u32,
    current_phase: String,
    phase_epoch: u64,
    planner_generation: u64,
    shop_revision: u32,
    decision_revision: u32,
    shop_visible_after: Option<Instant>,
    refreshes_used: u8,
    available_tribes: Vec<String>,
    current_shop: Vec<String>,
    stable_board: Vec<AgentCard>,
    last_snapshot: Option<AgentSnapshot>,
    last_fingerprint: Option<DecisionFingerprint>,
    open_choice_kind: Option<ChoiceKind>,
    open_choice_option_count: usize,
    active_choice_options: Vec<ChoiceOptionView>,
    trinket_choice_replan_pending: bool,
    trinket_ranking_generation: u64,
    trinket_option_signature: Vec<String>,
    trinket_rankings: Vec<TrinketOptionRecommendation>,
    trinket_ranking_busy: bool,
    compositions: Vec<CompositionOption>,
    selected_composition: Option<CompositionOption>,
    watchlist: Option<WatchlistResponse>,
    shop_hits: Vec<ShopHit>,
    decision_hits: Vec<ShopHit>,
    round_plan: Option<RoundPlan>,
    trinket_plan: Option<TrinketPlan>,
    tactical_plan: Option<TacticalPlan>,
    replan_events: Vec<ReplanEvent>,
    ai_status: String,
    error: Option<String>,
    usage: AiUsage,
    chat_generation: u64,
    chat_messages: Vec<ChatMessage>,
    chat_busy: bool,
    chat_status: String,
    overlay_test_mode: bool,
}

impl Default for MutableState {
    fn default() -> Self {
        Self {
            match_active: false,
            current_round: 0,
            current_phase: "Pregame".to_owned(),
            phase_epoch: 0,
            planner_generation: 0,
            shop_revision: 0,
            decision_revision: 0,
            shop_visible_after: None,
            refreshes_used: 0,
            available_tribes: Vec::new(),
            current_shop: Vec::new(),
            stable_board: Vec::new(),
            last_snapshot: None,
            last_fingerprint: None,
            open_choice_kind: None,
            open_choice_option_count: 0,
            active_choice_options: Vec::new(),
            trinket_choice_replan_pending: false,
            trinket_ranking_generation: 0,
            trinket_option_signature: Vec::new(),
            trinket_rankings: Vec::new(),
            trinket_ranking_busy: false,
            compositions: Vec::new(),
            selected_composition: None,
            watchlist: None,
            shop_hits: Vec::new(),
            decision_hits: Vec::new(),
            round_plan: None,
            trinket_plan: None,
            tactical_plan: None,
            replan_events: Vec::new(),
            ai_status: "等待对局".to_owned(),
            error: None,
            usage: AiUsage::default(),
            chat_generation: 0,
            chat_messages: Vec::new(),
            chat_busy: false,
            chat_status: "可以和 AI 交流想法或修正当前目标".to_owned(),
            overlay_test_mode: false,
        }
    }
}

#[derive(Debug, Default)]
struct ComplianceRuntime {
    session_id: Option<String>,
    session_started_at_ms: u64,
    session_ended_at_ms: Option<u64>,
    api_calls: Vec<ApiCallRecord>,
    trace: Vec<TraceEvent>,
    game_archive: Option<crate::harness::GameArchive>,
    loaded_session: Option<AgentSessionArchive>,
    next_api_call_id: u64,
}

#[derive(Clone)]
pub struct DemoState {
    inner: Arc<RwLock<MutableState>>,
    config: Arc<RwLock<DemoConfig>>,
    config_path: Arc<PathBuf>,
    catalog: Arc<CardCatalog>,
    knowledge: Arc<HdtKnowledgeBase>,
    compliance: Arc<RwLock<ComplianceRuntime>>,
    tasks: TaskManager,
    log_refresh_generation: Arc<AtomicU64>,
    watched_power_log: Arc<RwLock<Option<PathBuf>>>,
}

impl DemoState {
    pub fn new(config: DemoConfig, config_path: PathBuf, catalog: CardCatalog) -> Self {
        let knowledge = HdtKnowledgeBase::from_catalog(catalog.clone());
        let task_history_limit = config.compliance.task_history_limit;
        Self {
            inner: Arc::new(RwLock::new(MutableState::default())),
            config: Arc::new(RwLock::new(config)),
            config_path: Arc::new(config_path),
            catalog: Arc::new(catalog),
            knowledge: Arc::new(knowledge),
            compliance: Arc::new(RwLock::new(ComplianceRuntime::default())),
            tasks: TaskManager::new(task_history_limit),
            log_refresh_generation: Arc::new(AtomicU64::new(0)),
            watched_power_log: Arc::new(RwLock::new(None)),
        }
    }

    pub fn config(&self) -> DemoConfig {
        self.config.read().unwrap().clone()
    }

    pub fn environment_status(&self) -> EnvironmentStatus {
        inspect_environment(&self.config())
    }

    pub fn repair_environment(&self) -> Result<EnvironmentRepairResult, String> {
        let mut config = self.config.write().unwrap();
        let result = prepare_environment(&mut config).map_err(|error| error.to_string())?;
        if result.config_changed {
            config
                .save(self.config_path.as_ref().as_path())
                .map_err(|error| error.to_string())?;
        }
        Ok(result)
    }

    pub fn card_catalog_source(&self) -> String {
        self.catalog.source().unwrap_or("unknown").to_owned()
    }

    pub fn log_refresh_generation_handle(&self) -> Arc<AtomicU64> {
        self.log_refresh_generation.clone()
    }

    pub fn request_log_refresh(&self) -> u64 {
        let generation = self
            .log_refresh_generation
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1);
        self.tasks
            .cancel_all_running("用户刷新 Power.log，取消当前未完成任务");
        {
            let mut state = self.inner.write().unwrap();
            state.ai_status = "正在重新扫描最新 Power.log…".to_owned();
        }
        self.trace_event(
            "monitor",
            "用户刷新 Power.log",
            &format!("refresh_generation={generation}"),
            None,
        );
        generation
    }

    pub fn set_watched_power_log(&self, path: Option<PathBuf>) {
        *self.watched_power_log.write().unwrap() = path;
    }

    pub fn watched_power_log(&self) -> Option<PathBuf> {
        self.watched_power_log.read().unwrap().clone()
    }

    // ---------------------------------------------------------------------
    // V0.5.0 course compliance / desktop control center API
    // ---------------------------------------------------------------------

    pub fn task_records(&self) -> Vec<TaskRecord> {
        self.tasks.records()
    }

    pub fn cancel_task(&self, task_id: u64) -> bool {
        let cancelled = self.tasks.cancel(task_id);
        if cancelled {
            self.trace_event(
                "task",
                "用户取消任务",
                &format!("task_id={task_id}"),
                Some(task_id),
            );
        }
        cancelled
    }

    pub fn usage_summary(&self) -> UsageSummary {
        let config = self.config();
        let compliance = self.compliance.read().unwrap();
        let currency = compliance
            .api_calls
            .first()
            .map(|call| call.cost.currency.as_str())
            .unwrap_or(config.deepseek.pricing.currency.as_str());
        summarize_usage(&compliance.api_calls, &config.deepseek.budget, currency)
    }

    pub fn api_call_records(&self) -> Vec<ApiCallRecord> {
        self.compliance.read().unwrap().api_calls.clone()
    }

    pub fn trace_events(&self) -> Vec<TraceEvent> {
        self.compliance.read().unwrap().trace.clone()
    }

    pub fn current_session_id(&self) -> Option<String> {
        self.compliance.read().unwrap().session_id.clone()
    }

    pub fn loaded_session(&self) -> Option<AgentSessionArchive> {
        self.compliance.read().unwrap().loaded_session.clone()
    }

    pub fn session_summaries(&self) -> Result<Vec<SessionSummary>, String> {
        let config = self.config();
        list_sessions(&config.compliance.history_dir).map_err(|error| error.to_string())
    }

    pub fn load_session_for_review(&self, path: &Path) -> Result<AgentSessionArchive, String> {
        let session = load_session(path).map_err(|error| error.to_string())?;
        self.compliance.write().unwrap().loaded_session = Some(session.clone());
        Ok(session)
    }

    /// Restore the persisted Agent-side context for audit/replay work. The live
    /// Harness is never faked; this is only allowed when no match is active.
    pub fn restore_session_context(&self, path: &Path) -> Result<(), String> {
        if self.inner.read().unwrap().match_active {
            return Err("对局进行中不能恢复历史上下文；请结束当前对局后再试".to_owned());
        }
        let session = load_session(path).map_err(|error| error.to_string())?;
        {
            let mut state = self.inner.write().unwrap();
            state.available_tribes = session.available_tribes.clone();
            state.selected_composition = session.selected_composition.clone();
            state.watchlist = session.watchlist.clone();
            state.round_plan = session.round_plan.clone();
            state.trinket_plan = session.trinket_plan.clone();
            state.tactical_plan = session.tactical_plan.clone();
            state.replan_events = session.replan_events.clone();
            state.chat_messages = session.chat_messages.clone();
            state.usage = session.usage.usage.clone();
            state.ai_status = format!("已加载历史会话 {}（审计模式）", session.session_id);
        }
        {
            let mut compliance = self.compliance.write().unwrap();
            compliance.session_id = Some(session.session_id.clone());
            compliance.session_started_at_ms = session.started_at_ms;
            compliance.session_ended_at_ms = session.ended_at_ms;
            compliance.api_calls = session.api_calls.clone();
            compliance.trace = session.trace.clone();
            compliance.game_archive = session.game_archive.clone();
            compliance.loaded_session = Some(session);
        }
        Ok(())
    }

    pub fn save_current_session(&self) -> Result<PathBuf, String> {
        {
            let compliance = self.compliance.read().unwrap();
            if compliance.session_id.is_none() {
                return Err("当前没有可保存的对局/历史会话".to_owned());
            }
        }
        let frozen = {
            let compliance = self.compliance.read().unwrap();
            compliance
                .session_ended_at_ms
                .is_some()
                .then(|| compliance.loaded_session.clone())
                .flatten()
        };
        let archive = frozen.unwrap_or_else(|| self.build_session_archive(None));
        let config = self.config();
        let path = save_session(&config.compliance.history_dir, &archive)
            .map_err(|error| error.to_string())?;
        self.trace_event(
            "session",
            "会话已保存",
            &path.display().to_string(),
            None,
        );
        Ok(path)
    }

    pub fn update_model_config(&self, mut new_config: DeepSeekConfig) -> Result<(), String> {
        new_config.api_compatibility = new_config.api_compatibility.trim().to_ascii_lowercase();
        if !matches!(new_config.api_compatibility.as_str(), "deepseek" | "openai") {
            return Err("API模式只支持 deepseek 或 openai".to_owned());
        }
        new_config.base_url = new_config.base_url.trim().trim_end_matches('/').to_owned();
        if !(new_config.base_url.starts_with("http://") || new_config.base_url.starts_with("https://")) {
            return Err("Endpoint 必须以 http:// 或 https:// 开头".to_owned());
        }
        new_config.model = new_config.model.trim().to_owned();
        if new_config.model.is_empty() {
            return Err("Model 不能为空".to_owned());
        }
        new_config.context_window = new_config.context_window.clamp(1_024, 2_000_000);
        new_config.max_tokens = new_config.max_tokens.clamp(64, new_config.context_window);
        new_config.timeout_seconds = new_config.timeout_seconds.clamp(3, 600);
        new_config.pricing.currency = new_config.pricing.currency.trim().to_owned();
        if new_config.pricing.currency.is_empty() {
            return Err("货币名称不能为空，例如 CNY / USD".to_owned());
        }
        {
            let old_currency = self.config.read().unwrap().deepseek.pricing.currency.clone();
            let currency_changed = !old_currency.eq_ignore_ascii_case(&new_config.pricing.currency);
            if currency_changed {
                let active = self.inner.read().unwrap().match_active;
                let has_calls = !self.compliance.read().unwrap().api_calls.is_empty();
                if active && has_calls {
                    return Err(
                        "当前对局已经产生 API 成本；为避免混合不同货币，请在下一局再修改 currency"
                            .to_owned(),
                    );
                }
                if !active && has_calls {
                    let mut compliance = self.compliance.write().unwrap();
                    compliance.api_calls.clear();
                    compliance.next_api_call_id = 0;
                }
            }
        }
        for (label, value) in [
            ("输入价格", new_config.pricing.input_per_million),
            ("输出价格", new_config.pricing.output_per_million),
            ("缓存输入价格", new_config.pricing.cached_input_per_million),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(format!("{label} 必须是 >= 0 的有限数字"));
            }
        }
        if let Some(value) = new_config.budget.max_total_tokens {
            if value == 0 {
                new_config.budget.max_total_tokens = None;
            }
        }
        if let Some(value) = new_config.budget.max_cost {
            if !value.is_finite() {
                return Err("成本预算必须是有限数字".to_owned());
            }
            if value <= 0.0 {
                new_config.budget.max_cost = None;
            }
        }
        let mut config = self.config.write().unwrap();
        // The desktop UI resolves its masked sentinel before calling this method.
        // An actual empty string intentionally clears the saved key, which is
        // needed for local keyless OpenAI-compatible endpoints.
        config.deepseek = new_config;
        config
            .save(self.config_path.as_ref().as_path())
            .map_err(|error| error.to_string())?;
        drop(config);
        self.trace_event("config", "模型配置已更新", "Control Center 保存配置", None);
        Ok(())
    }

    pub fn request_model_test(&self) -> Result<u64, String> {
        let config = self.config();
        let task = self.start_ai_task("connection_test", "测试模型连接", None)?;
        let task_id = task.id();
        let client = self.client_for_task(&config, &task, "connection_test");
        let this = self.clone();
        thread::spawn(move || {
            task.progress("连接 Endpoint", 35, "正在建立 API 连接");
            match client.test_connection() {
                Ok((_message, usage)) => {
                    let mut state = this.inner.write().unwrap();
                    add_usage(&mut state.usage, &usage);
                    state.ai_status = "模型连接测试成功".to_owned();
                    drop(state);
                    task.complete("模型连接成功");
                    this.trace_event("task", "模型连接测试成功", "Endpoint 可用", Some(task_id));
                }
                Err(DeepSeekError::Cancelled) => {
                    task.cancelled("用户取消连接测试");
                }
                Err(error) => {
                    task.failed(&error.to_string());
                    this.inner.write().unwrap().error = Some(error.to_string());
                }
            }
        });
        Ok(task_id)
    }

    fn start_ai_task(
        &self,
        kind: &str,
        label: &str,
        round_number: Option<u32>,
    ) -> Result<TaskHandle, String> {
        let summary = self.usage_summary();
        if summary.budget_exhausted {
            self.tasks.cancel_all_running("Token/成本预算已达到上限");
            return Err("Token/成本预算已达到上限；请在 Control Center 调整预算后继续".to_owned());
        }
        let task = self.tasks.start(kind, label, round_number);
        self.trace_event("task", label, "任务已创建", Some(task.id()));
        Ok(task)
    }

    fn client_for_task(&self, config: &DemoConfig, task: &TaskHandle, kind: &str) -> DeepSeekClient {
        let this = self.clone();
        let model = config.deepseek.model.clone();
        let base_url = config.deepseek.base_url.clone();
        let pricing = config.deepseek.pricing.clone();
        let observer: ApiCallObserver = Arc::new(move |observation| {
            this.record_api_observation(
                observation,
                &model,
                &base_url,
                &pricing,
            );
        });
        DeepSeekClient::new(config.deepseek.clone()).with_task_control(
            task.id(),
            kind,
            task.cancel_flag(),
            observer,
        )
    }

    fn record_api_observation(
        &self,
        observation: ApiCallObservation,
        model: &str,
        base_url: &str,
        pricing: &super::config::PricingConfig,
    ) {
        let cost = calculate_cost(&observation.usage, pricing);
        let record = {
            let mut compliance = self.compliance.write().unwrap();
            compliance.next_api_call_id = compliance.next_api_call_id.saturating_add(1);
            let record = ApiCallRecord {
                id: compliance.next_api_call_id,
                task_id: observation.task_id,
                kind: observation.kind,
                model: model.to_owned(),
                base_url: base_url.to_owned(),
                started_at_ms: observation.started_at_ms,
                finished_at_ms: observation.finished_at_ms,
                status: observation.status,
                error: observation.error,
                usage: observation.usage,
                cost,
            };
            compliance.api_calls.push(record.clone());
            record
        };
        self.trace_event(
            "api",
            &format!("API · {}", record.kind),
            &format!(
                "input={} output={} total={} cost={:.6} {} status={}",
                record.usage.prompt_tokens,
                record.usage.completion_tokens,
                record.usage.total_tokens,
                record.cost.total_cost,
                record.cost.currency,
                record.status
            ),
            record.task_id,
        );
        let summary = self.usage_summary();
        if summary.budget_exhausted {
            self.tasks
                .cancel_all_running("Token/成本预算达到上限，自动中断剩余任务");
            let mut state = self.inner.write().unwrap();
            state.error = Some("Token/成本预算达到上限，AI 调用已自动停止".to_owned());
            state.ai_status = "预算已用尽 · AI 已停止".to_owned();
        }
    }

    fn trace_event(&self, category: &str, title: &str, detail: &str, task_id: Option<u64>) {
        let (round_number, phase) = {
            let state = self.inner.read().unwrap();
            (
                (state.current_round > 0).then_some(state.current_round),
                Some(state.current_phase.clone()),
            )
        };
        let mut compliance = self.compliance.write().unwrap();
        compliance.trace.push(TraceEvent {
            timestamp_ms: now_ms(),
            round_number,
            phase,
            category: category.to_owned(),
            title: title.to_owned(),
            detail: detail.to_owned(),
            task_id,
        });
        if compliance.trace.len() > 2_000 {
            let remove = compliance.trace.len() - 2_000;
            compliance.trace.drain(0..remove);
        }
    }

    fn build_session_archive(
        &self,
        game_archive_override: Option<crate::harness::GameArchive>,
    ) -> AgentSessionArchive {
        let config = self.config();
        let state = self.inner.read().unwrap();
        let compliance = self.compliance.read().unwrap();
        let game_archive = game_archive_override.or_else(|| compliance.game_archive.clone());
        let usage = summarize_usage(
            &compliance.api_calls,
            &config.deepseek.budget,
            &config.deepseek.pricing.currency,
        );
        AgentSessionArchive {
            schema_version: "0.5.0".to_owned(),
            session_id: compliance
                .session_id
                .clone()
                .unwrap_or_else(new_session_id),
            started_at_ms: compliance.session_started_at_ms,
            ended_at_ms: compliance.session_ended_at_ms,
            saved_at_ms: now_ms(),
            model: ModelConfigSnapshot::from(&config.deepseek),
            pricing: config.deepseek.pricing.clone(),
            budget: config.deepseek.budget.clone(),
            game_archive,
            available_tribes: state.available_tribes.clone(),
            selected_composition: state.selected_composition.clone(),
            watchlist: state.watchlist.clone(),
            round_plan: state.round_plan.clone(),
            trinket_plan: state.trinket_plan.clone(),
            tactical_plan: state.tactical_plan.clone(),
            replan_events: state.replan_events.clone(),
            chat_messages: state.chat_messages.clone(),
            api_calls: compliance.api_calls.clone(),
            tasks: self
                .tasks
                .records()
                .into_iter()
                .filter(|task| task.started_at_ms >= compliance.session_started_at_ms)
                .collect(),
            trace: compliance.trace.clone(),
            usage,
        }
    }

    fn finalize_session(&self, game_archive: crate::harness::GameArchive) {
        {
            let mut compliance = self.compliance.write().unwrap();
            compliance.session_ended_at_ms = Some(now_ms());
            compliance.game_archive = Some(game_archive.clone());
        }
        self.trace_event("session", "对局结束", "完整 Agent 会话已封存", None);

        // Freeze the final context before set_match_active(false) clears live-only
        // overlay state. Manual saves after the match reuse this immutable copy.
        let archive = self.build_session_archive(Some(game_archive));
        self.compliance.write().unwrap().loaded_session = Some(archive.clone());

        let config = self.config();
        if config.compliance.auto_save_sessions {
            if let Err(error) = save_session(&config.compliance.history_dir, &archive) {
                self.inner.write().unwrap().error = Some(format!("保存历史会话失败：{error}"));
            }
        }
    }

    fn finalize_interrupted_session(
        &self,
        game_archive: crate::harness::GameArchive,
        reason: &str,
    ) {
        {
            let mut compliance = self.compliance.write().unwrap();
            compliance.session_ended_at_ms = Some(now_ms());
            compliance.game_archive = Some(game_archive.clone());
        }
        self.trace_event(
            "session",
            "对局监控中断",
            &format!("Power.log source reset: {reason}"),
            None,
        );

        let archive = self.build_session_archive(Some(game_archive));
        self.compliance.write().unwrap().loaded_session = Some(archive.clone());
        let config = self.config();
        if config.compliance.auto_save_sessions {
            if let Err(error) = save_session(&config.compliance.history_dir, &archive) {
                self.inner.write().unwrap().error = Some(format!("保存中断会话失败：{error}"));
            }
        }
    }

    pub fn begin_match(&self) {
        {
            let mut state = self.inner.write().unwrap();
            *state = MutableState::default();
            state.match_active = true;
            state.phase_epoch = 1;
            state.ai_status = "对局已开始，等待种族信息".to_owned();
        }
        self.tasks.cancel_all_running("新对局开始，取消上一局未完成任务");
        {
            let mut compliance = self.compliance.write().unwrap();
            *compliance = ComplianceRuntime::default();
            compliance.session_id = Some(new_session_id());
            compliance.session_started_at_ms = now_ms();
        }
        self.trace_event("session", "对局开始", "创建新的完整 Agent 会话", None);
    }

    pub fn set_match_active(&self, active: bool) {
        if active {
            self.inner.write().unwrap().match_active = true;
            return;
        }

        {
            let mut state = self.inner.write().unwrap();
            let next_phase_epoch = state.phase_epoch.saturating_add(1);
            let next_planner_generation = state.planner_generation.saturating_add(1);
            let next_chat_generation = state.chat_generation.saturating_add(1);
            let next_trinket_generation = state.trinket_ranking_generation.saturating_add(1);
            *state = MutableState::default();
            state.current_phase = "Idle".to_owned();
            state.ai_status = "等待对局".to_owned();
            state.chat_status = "等待对局".to_owned();
            state.phase_epoch = next_phase_epoch;
            state.planner_generation = next_planner_generation;
            state.chat_generation = next_chat_generation;
            state.trinket_ranking_generation = next_trinket_generation;
        }
        self.tasks.cancel_all_running("对局结束或日志源刷新");
    }

    pub fn set_phase(&self, round_number: u32, phase: &str) {
        {
            let mut state = self.inner.write().unwrap();
            if state.current_phase != phase || state.current_round != round_number {
                state.phase_epoch = state.phase_epoch.saturating_add(1);
            }
            state.current_round = round_number;
            state.current_phase = phase.to_owned();
            if phase.eq_ignore_ascii_case("Recruit") {
                state.refreshes_used = 0;
                state.shop_revision = 0;
                state.decision_revision = 0;
                state.shop_visible_after = None;
            } else {
                state.current_shop.clear();
                state.shop_hits.clear();
                state.decision_hits.clear();
                state.decision_revision = 0;
                state.shop_visible_after = None;
                state.tactical_plan = None;
                state.overlay_test_mode = false;
            }
        }
        self.trace_event(
            "phase",
            &format!("Round {round_number} · {phase}"),
            "Harness phase boundary",
            None,
        );
    }

    pub fn set_available_tribes(&self, mut tribes: Vec<String>) {
        tribes.sort();
        tribes.dedup();
        let config = self.config();
        let should_auto_prepare = {
            let mut state = self.inner.write().unwrap();
            let changed = state.available_tribes != tribes;
            state.available_tribes = tribes;
            if state.match_active && state.ai_status.contains("等待种族") {
                state.ai_status = "可以让 AI 分析阵容".to_owned();
            }
            changed
                && state.match_active
                && !state.available_tribes.is_empty()
                && state.compositions.is_empty()
                && state.selected_composition.is_none()
                && state.watchlist.is_none()
                && config.agent.enabled
                && config.agent.auto_prepare_guide
                && !state.ai_status.starts_with("AI 正在分析可玩阵容")
        };
        // Do not keep the state lock while starting an async model task.
        if should_auto_prepare {
            if let Err(error) = self.request_analyze() {
                let mut state = self.inner.write().unwrap();
                state.error = Some(error.clone());
                state.ai_status = format!("阵容指南自动准备失败：{error}");
            }
        }
    }

    /// Compatibility helper used by tests/older callers. It no longer changes
    /// phase; a ShopUpdated is accepted only inside the matching Recruit phase.
    pub fn set_shop(&self, round_number: u32, card_ids: Vec<String>) {
        self.set_shop_revision(round_number, 0, card_ids);
    }

    pub fn set_shop_revision(&self, round_number: u32, revision: u32, card_ids: Vec<String>) -> bool {
        let mut state = self.inner.write().unwrap();
        if !state.match_active
            || !state.current_phase.eq_ignore_ascii_case("Recruit")
            || state.current_round != round_number
        {
            return false;
        }
        state.shop_revision = revision.max(state.shop_revision);
        state.current_shop = card_ids;
        // Atomic invalidation: a new semantic shop can never be drawn with the
        // previous shop's decision hits. Tactical recomputation will publish a
        // matching decision_revision later.
        state.decision_hits.clear();
        state.decision_revision = 0;
        state.tactical_plan = None;
        let delay_ms = self.config.read().unwrap().overlay.highlight_delay_ms;
        state.shop_visible_after = Some(Instant::now() + Duration::from_millis(delay_ms));
        recompute_hits_locked(&mut state, &self.catalog);
        true
    }

    pub fn clear_shop(&self) {
        let mut state = self.inner.write().unwrap();
        state.current_shop.clear();
        state.shop_hits.clear();
        state.decision_hits.clear();
        state.decision_revision = 0;
        state.shop_visible_after = None;
        state.tactical_plan = None;
    }

    /// Main bridge from the semantic Harness to the Agent. This method is the
    /// only place where live events are translated into planning/execution work.
    pub fn observe_runtime(&self, event: &HarnessEvent, runtime: &HarnessRuntime) {
        match event {
            HarnessEvent::MatchStarted => self.begin_match(),
            HarnessEvent::MatchInterrupted { reason } => {
                self.tasks
                    .cancel_all_running("Power.log 日志源刷新，封存当前会话");
                let _ = self.tasks.wait_for_idle(Duration::from_millis(300));
                self.finalize_interrupted_session(runtime.archive().clone(), reason);
                self.set_match_active(false);
                return;
            }
            HarnessEvent::MatchCompleted => {
                // Cancel any late planner/chat request first and give the async
                // HTTP futures a short window to publish their final cancelled
                // status/usage before freezing the session JSON.
                self.tasks.cancel_all_running("对局结束，封存会话");
                let _ = self.tasks.wait_for_idle(Duration::from_millis(800));
                self.finalize_session(runtime.archive().clone());
                self.set_match_active(false);
                return;
            }
            HarnessEvent::PhaseStarted { round_number, phase } => {
                let phase_name = phase_name(*phase);
                self.set_phase(*round_number, phase_name);
            }
            HarnessEvent::ShopUpdated { round_number, revision, card_ids } => {
                if !self.set_shop_revision(*round_number, *revision, card_ids.clone()) {
                    return;
                }
            }
            HarnessEvent::RecruitAction { kind, .. } => {
                if matches!(kind, RecruitActionKind::Refresh) {
                    let mut state = self.inner.write().unwrap();
                    state.refreshes_used = state.refreshes_used.saturating_add(1);
                }
            }
            HarnessEvent::ChoiceOpened { .. } | HarnessEvent::ChoiceUpdated { .. } => {
                let current = runtime.current_choice();
                let option_views = current
                    .as_ref()
                    .map(choice_option_views)
                    .unwrap_or_default();
                let mut state = self.inner.write().unwrap();
                state.open_choice_kind = current.as_ref().map(|choice| choice.kind.clone());
                state.open_choice_option_count = option_views.len();
                state.active_choice_options = option_views;
                if state.open_choice_kind == Some(ChoiceKind::Trinket) {
                    state.ai_status = format!(
                        "已识别饰品选择 · {} 个候选",
                        state.open_choice_option_count
                    );
                }
            }
            HarnessEvent::ChoiceResolved { .. } | HarnessEvent::RoundArchived { .. } => {}
        }

        let tribes = self.inner.read().unwrap().available_tribes.clone();
        let mut snapshot = AgentSnapshot::capture(runtime, &tribes);

        // The right-side lineup is a Recruit ownership view, not the temporary
        // combat simulation. Keep it frozen until the next stable Recruit state.
        let phase = self.inner.read().unwrap().current_phase.clone();
        if phase.eq_ignore_ascii_case("Combat") {
            let stable = self.inner.read().unwrap().stable_board.clone();
            if !stable.is_empty() {
                snapshot.board = stable;
            }
        }

        if phase.eq_ignore_ascii_case("Recruit") && matches!(event, HarnessEvent::ShopUpdated { .. }) {
            self.process_recruit_snapshot(snapshot.clone());
        } else {
            self.inner.write().unwrap().last_snapshot = Some(snapshot.clone());
        }

        match event {
            HarnessEvent::PhaseStarted { round_number, phase: PhaseKind::Combat } => {
                let config = self.config();
                if config.agent.enabled && config.agent.auto_plan_during_combat {
                    self.spawn_strategic_plan(snapshot, round_number.saturating_add(1), Vec::new());
                }
            }
            HarnessEvent::PhaseStarted { round_number, phase: PhaseKind::Recruit } => {
                self.ensure_plan_for_round(snapshot, *round_number);
            }
            HarnessEvent::ShopUpdated { .. } => {
                self.recompute_tactical();
            }
            HarnessEvent::RecruitAction { kind, .. } => {
                if matches!(kind, RecruitActionKind::TavernUpgrade) {
                    self.push_replan_and_maybe_spawn(
                        snapshot,
                        ReplanEvent::new(ReplanLevel::Strategic, "tavern_upgrade", "玩家刚刚升级了酒馆"),
                    );
                } else if !matches!(kind, RecruitActionKind::Refresh) {
                    self.recompute_tactical();
                }
            }
            HarnessEvent::ChoiceOpened { .. } | HarnessEvent::ChoiceUpdated { .. } => {
                let (trinket, current, should_request) = {
                    let mut state = self.inner.write().unwrap();
                    let trinket = state.open_choice_kind == Some(ChoiceKind::Trinket);
                    let current = state.current_round;
                    let has_preplan = state
                        .trinket_plan
                        .as_ref()
                        .map(|plan| plan.target_round == current)
                        .unwrap_or(false);
                    let should_request = trinket
                        && !has_preplan
                        && !state.trinket_choice_replan_pending;
                    if should_request {
                        state.trinket_choice_replan_pending = true;
                    }
                    (trinket, current, should_request)
                };
                if trinket && should_request {
                    self.spawn_strategic_plan(
                        snapshot,
                        current,
                        vec![ReplanEvent::new(
                            ReplanLevel::Strategic,
                            "unexpected_trinket_choice",
                            "检测到未提前命中的饰品选择时点，立即生成作用优先级",
                        )],
                    );
                }
                if trinket {
                    self.schedule_trinket_ranking();
                }
            }
            HarnessEvent::ChoiceResolved { .. } => {
                let trinket = {
                    let mut state = self.inner.write().unwrap();
                    state.open_choice_option_count = 0;
                    state.active_choice_options.clear();
                    state.trinket_choice_replan_pending = false;
                    state.trinket_ranking_generation = state.trinket_ranking_generation.saturating_add(1);
                    state.trinket_option_signature.clear();
                    state.trinket_rankings.clear();
                    state.trinket_ranking_busy = false;
                    state.open_choice_kind.take() == Some(ChoiceKind::Trinket)
                };
                if trinket {
                    self.push_replan_and_maybe_spawn(
                        snapshot,
                        ReplanEvent::new(ReplanLevel::Strategic, "trinket_selected", "饰品选择完成，重新检查本回合小目标"),
                    );
                }
            }
            _ => {}
        }
    }

    fn process_recruit_snapshot(&self, snapshot: AgentSnapshot) {
        let config = self.config();
        let selected_core_cards = self
            .inner
            .read()
            .unwrap()
            .selected_composition
            .as_ref()
            .map(|item| item.core_card_ids.clone())
            .unwrap_or_default();
        let now = DecisionFingerprint::from_snapshot(&snapshot);
        let events = {
            let state = self.inner.read().unwrap();
            state
                .last_fingerprint
                .as_ref()
                .map(|before| {
                    detect_replan(
                        before,
                        &now,
                        &selected_core_cards,
                        config.agent.critical_health,
                        config.agent.emergency_health,
                    )
                })
                .unwrap_or_default()
        };
        {
            let mut state = self.inner.write().unwrap();
            state.stable_board = snapshot.board.clone();
            state.last_snapshot = Some(snapshot.clone());
            state.last_fingerprint = Some(now);
            append_replan_events(&mut state.replan_events, &events);
        }
        if events.iter().any(|event| event.level >= ReplanLevel::Strategic) {
            let target_round = self.inner.read().unwrap().current_round;
            self.spawn_strategic_plan(snapshot, target_round, events);
        }
    }

    fn ensure_plan_for_round(&self, snapshot: AgentSnapshot, round_number: u32) {
        // Combat-start planning happens before combat damage is known. At the
        // Recruit boundary, compare the newly settled hero/tier/trinket facts
        // against the last Recruit fingerprint and immediately invalidate the
        // pre-plan if the actual combat outcome changed the risk regime.
        let config = self.config();
        let selected_core_cards = self
            .inner
            .read()
            .unwrap()
            .selected_composition
            .as_ref()
            .map(|item| item.core_card_ids.clone())
            .unwrap_or_default();
        let now = DecisionFingerprint::from_snapshot(&snapshot);
        let boundary_events = {
            let state = self.inner.read().unwrap();
            state.last_fingerprint.as_ref().map(|before| {
                detect_replan(
                    before,
                    &now,
                    &selected_core_cards,
                    config.agent.critical_health,
                    config.agent.emergency_health,
                )
            }).unwrap_or_default()
        };
        {
            let mut state = self.inner.write().unwrap();
            state.last_fingerprint = Some(now);
            append_replan_events(&mut state.replan_events, &boundary_events);
        }
        if boundary_events.iter().any(|event| event.level >= ReplanLevel::Strategic) {
            self.spawn_strategic_plan(snapshot, round_number, boundary_events);
            return;
        }

        let has_current = self
            .inner
            .read()
            .unwrap()
            .round_plan
            .as_ref()
            .map(|plan| plan.target_round == round_number)
            .unwrap_or(false);
        if !has_current {
            self.spawn_strategic_plan(snapshot, round_number, vec![ReplanEvent::new(
                ReplanLevel::Strategic,
                "missing_round_plan",
                "Recruit 已开始但没有对应回合计划，立即补计划",
            )]);
        } else {
            self.recompute_tactical();
        }
    }

    fn push_replan_and_maybe_spawn(&self, snapshot: AgentSnapshot, event: ReplanEvent) {
        {
            let mut state = self.inner.write().unwrap();
            append_replan_events(&mut state.replan_events, std::slice::from_ref(&event));
        }
        if event.level >= ReplanLevel::Strategic {
            let target = self.inner.read().unwrap().current_round;
            self.spawn_strategic_plan(snapshot, target, vec![event]);
        } else {
            self.recompute_tactical();
        }
    }

    fn spawn_strategic_plan(&self, snapshot: AgentSnapshot, target_round: u32, replan_events: Vec<ReplanEvent>) {
        let config = self.config();
        if !config.agent.enabled || target_round == 0 {
            return;
        }
        let need_trinket_plan = config.agent.trinket_rounds.contains(&target_round)
            || replan_events.iter().any(|event| {
                matches!(event.code.as_str(), "unexpected_trinket_choice" | "trinket_selected")
            });
        let (generation, selected, watchlist, previous) = {
            let mut state = self.inner.write().unwrap();
            state.planner_generation = state.planner_generation.saturating_add(1);
            let generation = state.planner_generation;
            state.error = None;
            state.ai_status = if snapshot.phase.eq_ignore_ascii_case("Combat") {
                format!("Combat 思考第 {target_round} 回合小目标…")
            } else {
                format!("重大事件触发 Replan：第 {target_round} 回合…")
            };
            (
                generation,
                state.selected_composition.clone(),
                state.watchlist.clone(),
                state.round_plan.clone(),
            )
        };

        let api_allowed = !(config.deepseek.api_key.trim().is_empty()
            && config.deepseek.api_compatibility.eq_ignore_ascii_case("deepseek"));
        let (task, client, task_start_error) = if api_allowed {
            match self.start_ai_task(
                "strategic_plan",
                &format!("规划第 {target_round} 回合"),
                Some(target_round),
            ) {
                Ok(task) => {
                    let client = self.client_for_task(&config, &task, "strategic_plan");
                    (Some(task), Some(client), None)
                }
                Err(error) => (None, None, Some(error)),
            }
        } else {
            (None, None, None)
        };

        let this = self.clone();
        thread::spawn(move || {
            if let Some(task) = task.as_ref() {
                task.progress("构建稳定状态", 20, "整理当前阵容、血量、酒馆等级和重大事件");
                task.progress("请求模型", 45, "等待慢速战略规划；可在 Control Center 取消");
            }
            let result = if let Some(client) = client.as_ref() {
                client.plan_next_recruit(
                    &snapshot,
                    selected.as_ref(),
                    watchlist.as_ref(),
                    previous.as_ref(),
                    &replan_events,
                    target_round,
                    need_trinket_plan,
                )
            } else {
                Ok((
                    fallback_strategic_plan(&snapshot, target_round, need_trinket_plan),
                    AiUsage::default(),
                ))
            };

            let (plan_response, usage, fallback_error, was_cancelled) = match result {
                Ok((response, usage)) => (response, usage, task_start_error, false),
                Err(DeepSeekError::Cancelled) => (
                    fallback_strategic_plan(&snapshot, target_round, need_trinket_plan),
                    AiUsage::default(),
                    Some("用户取消慢规划，已使用本地兜底".to_owned()),
                    true,
                ),
                Err(error) => (
                    fallback_strategic_plan(&snapshot, target_round, need_trinket_plan),
                    AiUsage::default(),
                    Some(error.to_string()),
                    false,
                ),
            };
            let goal_for_trace = plan_response.round_plan.primary_goal.clone();
            let plan_trace_json = serde_json::to_string_pretty(&plan_response)
                .unwrap_or_else(|_| format!("小目标：{}", goal_for_trace));

            if let Some(task) = task.as_ref() {
                if was_cancelled {
                    task.cancelled("用户取消慢规划；本地兜底已接管");
                } else if let Some(error) = fallback_error.as_ref() {
                    task.failed(&format!("{error}；本地兜底已接管"));
                } else {
                    task.progress("Rust 校验", 85, "规范化权重、升本倾向和 Replan 条件");
                    task.complete("下一回合小目标已生成");
                }
            }

            {
                let mut state = this.inner.write().unwrap();
                // Epoch/generation fence: stale async planner responses can never
                // overwrite a newer replan.
                if generation != state.planner_generation || !state.match_active {
                    return;
                }
                state.round_plan = Some(plan_response.round_plan);
                state.trinket_plan = plan_response.trinket_plan;
                if need_trinket_plan {
                    state.trinket_choice_replan_pending = false;
                }
                add_usage(&mut state.usage, &usage);
                if let Some(error) = fallback_error.as_ref() {
                    state.error = Some(format!("慢规划器失败/取消，已使用本地兜底：{error}"));
                    state.ai_status = format!("第 {target_round} 回合计划已用本地兜底生成");
                } else {
                    state.ai_status = format!("第 {target_round} 回合小目标已生成");
                }
            }
            this.trace_event(
                "planner",
                &format!("第 {target_round} 回合战略计划"),
                &format!(
                    "{}{}",
                    plan_trace_json,
                    fallback_error
                        .as_ref()
                        .map(|error| format!("\n[fallback] {error}"))
                        .unwrap_or_default()
                ),
                task.as_ref().map(TaskHandle::id),
            );
            this.recompute_tactical();
            this.schedule_trinket_ranking();
        });
    }

    fn schedule_trinket_ranking(&self) {
        let config = self.config();
        let generation = {
            let mut state = self.inner.write().unwrap();
            if state.open_choice_kind != Some(ChoiceKind::Trinket)
                || state.active_choice_options.is_empty()
            {
                return;
            }
            let mut signature = state
                .active_choice_options
                .iter()
                .map(|option| format!("{}|{}|{}", option.card_id, option.name, option.text))
                .collect::<Vec<_>>();
            if let Some(plan) = state.trinket_plan.as_ref() {
                signature.push(format!(
                    "plan:{}|{:?}",
                    plan.desired_effect,
                    plan.role_priorities
                        .iter()
                        .map(|item| (&item.role, item.weight))
                        .collect::<Vec<_>>()
                ));
            }
            if signature == state.trinket_option_signature
                && (state.trinket_ranking_busy || !state.trinket_rankings.is_empty())
            {
                return;
            }
            state.trinket_option_signature = signature;
            state.trinket_ranking_generation = state.trinket_ranking_generation.saturating_add(1);
            state.trinket_ranking_busy = true;
            state.trinket_rankings.clear();
            state.trinket_ranking_generation
        };

        let round = self.inner.read().unwrap().current_round;
        let api_allowed = !(config.deepseek.api_key.trim().is_empty()
            && config.deepseek.api_compatibility.eq_ignore_ascii_case("deepseek"));
        let (task, client, task_start_error) = if api_allowed {
            match self.start_ai_task("trinket_rank", "饰品候选排序", Some(round)) {
                Ok(task) => {
                    let client = self.client_for_task(&config, &task, "trinket_rank");
                    (Some(task), Some(client), None)
                }
                Err(error) => (None, None, Some(error)),
            }
        } else {
            (None, None, None)
        };

        let this = self.clone();
        thread::spawn(move || {
            // ChoiceUpdated can arrive once per option. Debounce so only the
            // last settled candidate list reaches the model.
            thread::sleep(Duration::from_millis(220));
            let (options, round_plan, trinket_plan, still_valid) = {
                let state = this.inner.read().unwrap();
                (
                    state.active_choice_options.clone(),
                    state.round_plan.clone(),
                    state.trinket_plan.clone(),
                    generation == state.trinket_ranking_generation
                        && state.open_choice_kind == Some(ChoiceKind::Trinket)
                        && !state.active_choice_options.is_empty(),
                )
            };
            if !still_valid {
                if let Some(task) = task.as_ref() {
                    task.cancelled("饰品候选发生变化，旧排序任务作废");
                }
                return;
            }

            if let Some(task) = task.as_ref() {
                task.progress("读取实际候选", 25, "使用 Harness 识别出的饰品名和卡牌文本");
                task.progress("请求模型", 50, "仅在当前真实候选中排序");
            }
            let fallback = || fallback_trinket_rankings(&options, trinket_plan.as_ref());
            let (rankings, usage, error, cancelled) = if let Some(client) = client.as_ref() {
                match client.rank_trinket_options(
                    &options,
                    round_plan.as_ref(),
                    trinket_plan.as_ref(),
                ) {
                    Ok((response, usage)) => (response.recommendations, usage, None, false),
                    Err(DeepSeekError::Cancelled) => (
                        fallback(),
                        AiUsage::default(),
                        Some("用户取消饰品排序".to_owned()),
                        true,
                    ),
                    Err(error) => (
                        fallback(),
                        AiUsage::default(),
                        Some(error.to_string()),
                        false,
                    ),
                }
            } else {
                (fallback(), AiUsage::default(), task_start_error, false)
            };
            if let Some(task) = task.as_ref() {
                if cancelled {
                    task.cancelled("用户取消饰品排序；本地排序继续可用");
                } else if let Some(error) = error.as_ref() {
                    task.failed(&format!("{error}；已显示本地排序"));
                } else {
                    task.complete("饰品候选排序完成");
                }
            }
            let top_name = rankings.first().map(|item| item.name.clone()).unwrap_or_default();
            let mut state = this.inner.write().unwrap();
            if generation != state.trinket_ranking_generation
                || state.open_choice_kind != Some(ChoiceKind::Trinket)
            {
                return;
            }
            state.trinket_rankings = rankings;
            state.trinket_ranking_busy = false;
            add_usage(&mut state.usage, &usage);
            state.ai_status = if let Some(error) = error.as_ref() {
                format!("饰品候选已识别，AI排序未完成：{error}")
            } else {
                "饰品候选已按当前计划排序".to_owned()
            };
            drop(state);
            let ranking_trace = {
                let state = this.inner.read().unwrap();
                serde_json::to_string_pretty(&state.trinket_rankings).unwrap_or_else(|_| {
                    format!("首选：{}", if top_name.is_empty() { "本地候选" } else { &top_name })
                })
            };
            this.trace_event(
                "planner",
                "饰品规划",
                &ranking_trace,
                task.as_ref().map(TaskHandle::id),
            );
        });
    }

    fn recompute_tactical(&self) {
        let (snapshot, round_plan, trinket_plan, watchlist, refreshes, revision, phase, round, shop_empty) = {
            let state = self.inner.read().unwrap();
            (
                state.last_snapshot.clone(),
                state.round_plan.clone(),
                state.trinket_plan.clone(),
                state.watchlist.clone(),
                state.refreshes_used,
                state.shop_revision,
                state.current_phase.clone(),
                state.current_round,
                state.current_shop.is_empty(),
            )
        };
        if !phase.eq_ignore_ascii_case("Recruit") || shop_empty {
            return;
        }
        let Some(mut snapshot) = snapshot else { return; };
        if snapshot.round_number != round {
            snapshot.round_number = round;
        }
        let tactical = evaluate_recruit(
            &snapshot,
            round_plan.as_ref().filter(|plan| plan.target_round == round),
            trinket_plan.as_ref().filter(|plan| plan.target_round == round),
            watchlist.as_ref(),
            &self.catalog,
            refreshes,
            revision,
        );
        let decision_hits = tactical_hits(&tactical, &snapshot, &self.catalog);
        let trace_detail = tactical
            .ranked_actions
            .iter()
            .take(4)
            .map(|action| format!("{:.1} {}", action.score, action.label))
            .collect::<Vec<_>>()
            .join(" | ");
        let mut published = false;
        let mut state = self.inner.write().unwrap();
        if state.current_phase.eq_ignore_ascii_case("Recruit")
            && state.current_round == round
            && tactical.shop_revision == state.shop_revision
        {
            state.decision_revision = tactical.shop_revision;
            state.tactical_plan = Some(tactical);
            state.decision_hits = decision_hits;
            published = true;
        }
        drop(state);
        if published {
            self.trace_event(
                "tactical",
                &format!("Round {round} Tactical #{}", revision),
                &trace_detail,
                None,
            );
        }
    }

    pub fn public_state(&self) -> PublicState {
        let state = self.inner.read().unwrap();
        let config = self.config.read().unwrap();
        let current_stage = stage_for_round(state.current_round);
        let current_stage_recommendation_count = state
            .watchlist
            .as_ref()
            .and_then(|watchlist| watchlist.stages.iter().find(|stage| stage.stage == current_stage))
            .map(|stage| stage.cards.len())
            .unwrap_or(0);
        let current_pool_count = self.knowledge.current_pool_count(&state.available_tribes);
        let current_shop = state
            .current_shop
            .iter()
            .enumerate()
            .map(|(index, card_id)| {
                let resolved = self.catalog.resolve(card_id);
                ShopCardView {
                    index,
                    card_id: card_id.clone(),
                    name: resolved.as_ref().and_then(|card| card.preferred_name()).unwrap_or(card_id).to_owned(),
                    tavern_tier: resolved.as_ref().and_then(|card| card.tavern_tier()),
                    text: resolved.as_ref().and_then(|card| card.preferred_text()).unwrap_or("").to_owned(),
                }
            })
            .collect();
        PublicState {
            match_active: state.match_active,
            current_round: state.current_round,
            current_phase: state.current_phase.clone(),
            phase_epoch: state.phase_epoch,
            shop_revision: state.shop_revision,
            decision_revision: state.decision_revision,
            overlay_marks_ready: state
                .shop_visible_after
                .as_ref()
                .map(|ready| Instant::now() >= *ready)
                .unwrap_or(false),
            current_stage: current_stage.to_owned(),
            available_tribes: state.available_tribes.clone(),
            current_shop,
            stable_board: state.stable_board.clone(),
            compositions: state.compositions.clone(),
            selected_composition: state.selected_composition.clone(),
            watchlist: state.watchlist.clone(),
            shop_hits: state.shop_hits.clone(),
            decision_hits: state.decision_hits.clone(),
            round_plan: state.round_plan.clone(),
            trinket_plan: state.trinket_plan.clone(),
            tactical_plan: state.tactical_plan.clone(),
            replan_events: state.replan_events.clone(),
            active_choice_kind: state.open_choice_kind.clone(),
            active_choice_option_count: state.open_choice_option_count,
            active_choice_options: state.active_choice_options.clone(),
            trinket_rankings: state.trinket_rankings.clone(),
            trinket_ranking_busy: state.trinket_ranking_busy,
            ai_status: state.ai_status.clone(),
            error: state.error.clone(),
            api_key_configured: !config.deepseek.api_key.trim().is_empty()
                || !config.deepseek.api_compatibility.eq_ignore_ascii_case("deepseek"),
            model: config.deepseek.model.clone(),
            base_url: config.deepseek.base_url.clone(),
            usage: state.usage.clone(),
            knowledge: self.knowledge.stats().clone(),
            current_pool_count,
            current_stage_recommendation_count,
            overlay_test_mode: state.overlay_test_mode,
            chat_messages: state.chat_messages.clone(),
            chat_busy: state.chat_busy,
            chat_status: state.chat_status.clone(),
            tasks: self.tasks.records(),
            usage_summary: {
                let compliance = self.compliance.read().unwrap();
                summarize_usage(
                    &compliance.api_calls,
                    &config.deepseek.budget,
                    &config.deepseek.pricing.currency,
                )
            },
            session_id: self.compliance.read().unwrap().session_id.clone(),
            trace_tail: self
                .compliance
                .read()
                .unwrap()
                .trace
                .iter()
                .rev()
                .take(40)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
        }
    }

    pub fn overlay_hits(&self) -> (Vec<String>, Vec<ShopHit>) {
        let state = self.inner.read().unwrap();
        let hits = if state.tactical_plan.is_some() {
            if state.decision_revision == state.shop_revision {
                state.decision_hits.clone()
            } else {
                Vec::new()
            }
        } else {
            state.shop_hits.clone()
        };
        (state.current_shop.clone(), hits)
    }

    pub fn request_analyze(&self) -> Result<(), String> {
        {
            let state = self.inner.read().unwrap();
            if state.available_tribes.is_empty() {
                return Err("尚未获取本局可用种族，请进入酒馆战棋对局后再试".to_owned());
            }
        }
        let config = self.config();
        if config.deepseek.api_key.trim().is_empty()
            && config.deepseek.api_compatibility.eq_ignore_ascii_case("deepseek")
        {
            return Err("请先在 HearthCoach Control Center 配置 API Key".to_owned());
        }
        let round = self.inner.read().unwrap().current_round;
        let task = self.start_ai_task("composition_analysis", "分析可玩阵容", (round > 0).then_some(round))?;
        let client = self.client_for_task(&config, &task, "composition_analysis");
        {
            let mut state = self.inner.write().unwrap();
            state.ai_status = "AI 正在分析可玩阵容…".to_owned();
            state.error = None;
        }
        let this = self.clone();
        thread::spawn(move || {
            task.progress("构建 HDT 上下文", 20, "整理当前可用种族和真实卡池");
            let tribes = this.inner.read().unwrap().available_tribes.clone();
            if task.is_cancelled() {
                task.cancelled("用户在 API 调用前取消");
                return;
            }
            task.progress("请求模型", 45, "等待模型生成 3~4 个阵容方向");
            match client.suggest_compositions(&tribes, &this.knowledge) {
                Ok((response, usage)) => {
                    task.progress("Rust 校验", 85, "校验核心 CardId 是否属于当前 HDT 卡池");
                    let names = response
                        .compositions
                        .iter()
                        .map(|item| item.name.as_str())
                        .collect::<Vec<_>>()
                        .join(" / ");
                    let auto_prepare = this.config().agent.auto_prepare_guide;
                    let first_id = response.compositions.first().map(|item| item.id.clone());
                    let mut state = this.inner.write().unwrap();
                    state.compositions = response.compositions;
                    state.selected_composition = None;
                    state.watchlist = None;
                    state.shop_hits.clear();
                    add_usage(&mut state.usage, &usage);
                    state.ai_status = if auto_prepare && first_id.is_some() {
                        "阵容方向已生成；正在自动准备阵容指南…".to_owned()
                    } else {
                        "请选择一个阵容方向".to_owned()
                    };
                    drop(state);
                    task.complete("阵容方向已生成并通过 HDT 校验");
                    this.trace_event(
                        "planner",
                        "阵容分析完成",
                        &format!("候选：{names}"),
                        Some(task.id()),
                    );
                    if auto_prepare {
                        if let Some(first_id) = first_id {
                            if let Err(error) = this.request_select(&first_id) {
                                let mut state = this.inner.write().unwrap();
                                state.error = Some(error.clone());
                                state.ai_status = format!("自动生成阵容指南失败：{error}");
                            }
                        }
                    }
                }
                Err(DeepSeekError::Cancelled) => {
                    task.cancelled("用户取消阵容分析");
                    this.inner.write().unwrap().ai_status = "阵容分析已取消".to_owned();
                }
                Err(error) => {
                    task.failed(&error.to_string());
                    let mut state = this.inner.write().unwrap();
                    state.error = Some(error.to_string());
                    state.ai_status = "AI 请求失败".to_owned();
                }
            }
        });
        Ok(())
    }

    pub fn request_select(&self, id: &str) -> Result<(), String> {
        let selected = {
            let state = self.inner.read().unwrap();
            state
                .compositions
                .iter()
                .find(|item| item.id == id)
                .cloned()
                .ok_or_else(|| "找不到这个阵容选项".to_owned())?
        };
        let config = self.config();
        if config.deepseek.api_key.trim().is_empty()
            && config.deepseek.api_compatibility.eq_ignore_ascii_case("deepseek")
        {
            return Err("请先在 HearthCoach Control Center 配置 API Key".to_owned());
        }
        let round = self.inner.read().unwrap().current_round;
        let task = self.start_ai_task("watchlist", "生成阵容指南", (round > 0).then_some(round))?;
        let client = self.client_for_task(&config, &task, "watchlist");
        {
            let mut state = self.inner.write().unwrap();
            state.selected_composition = Some(selected.clone());
            state.watchlist = None;
            state.shop_hits.clear();
            state.decision_hits.clear();
            state.planner_generation = state.planner_generation.saturating_add(1);
            state.round_plan = None;
            state.trinket_plan = None;
            state.ai_status = format!("AI 正在为「{}」生成前中后期卡表…", selected.name);
            state.error = None;
        }
        let this = self.clone();
        thread::spawn(move || {
            task.progress("读取 HDT", 20, "构建当前版本事实卡池");
            let tribes = this.inner.read().unwrap().available_tribes.clone();
            task.progress("请求模型", 45, "生成前期/中期/后期 Watchlist");
            match client.build_watchlist(&tribes, &selected, &this.knowledge) {
                Ok((watchlist, usage)) => {
                    task.progress("Rust 校验", 85, "验证每张推荐牌 CardId 和酒馆等级");
                    let mut state = this.inner.write().unwrap();
                    state.watchlist = Some(watchlist);
                    add_usage(&mut state.usage, &usage);
                    state.ai_status = "卡牌观察列表已生成；动态决策会在其上叠加".to_owned();
                    recompute_hits_locked(&mut state, &this.catalog);
                    let snapshot = state.last_snapshot.clone();
                    let round = state.current_round;
                    drop(state);
                    task.complete("阵容指南已生成并通过 HDT 校验");
                    let watchlist_trace = this
                        .inner
                        .read()
                        .unwrap()
                        .watchlist
                        .as_ref()
                        .and_then(|watchlist| serde_json::to_string_pretty(watchlist).ok())
                        .unwrap_or_else(|| format!("阵容：{}", selected.name));
                    this.trace_event(
                        "planner",
                        "阵容指南生成",
                        &watchlist_trace,
                        Some(task.id()),
                    );
                    if let Some(snapshot) = snapshot {
                        if round > 0 {
                            this.spawn_strategic_plan(
                                snapshot,
                                round,
                                vec![ReplanEvent::new(
                                    ReplanLevel::Strategic,
                                    "composition_selected",
                                    "玩家选择了阵容方向并生成新 Watchlist",
                                )],
                            );
                        }
                    }
                    this.recompute_tactical();
                }
                Err(DeepSeekError::Cancelled) => {
                    task.cancelled("用户取消阵容指南生成");
                    this.inner.write().unwrap().ai_status = "阵容指南生成已取消".to_owned();
                }
                Err(error) => {
                    task.failed(&error.to_string());
                    let mut state = this.inner.write().unwrap();
                    state.error = Some(error.to_string());
                    state.ai_status = "AI 请求失败".to_owned();
                }
            }
        });
        Ok(())
    }

    /// Send one in-match natural-language message to the coach. Ordinary
    /// discussion leaves the plan untouched; an explicit strategy correction
    /// may return a validated RoundPlanPatch and immediately re-score Recruit.
    pub fn request_chat(&self, message: &str) -> Result<(), String> {
        let message = message.trim();
        if message.is_empty() {
            return Err("请输入想和 AI 交流的内容".to_owned());
        }
        if message.chars().count() > 500 {
            return Err("单条消息请控制在 500 字以内".to_owned());
        }
        let config = self.config();
        if config.deepseek.api_key.trim().is_empty()
            && config.deepseek.api_compatibility.eq_ignore_ascii_case("deepseek")
        {
            return Err("请先在 HearthCoach Control Center 配置 API Key".to_owned());
        }
        {
            let state = self.inner.read().unwrap();
            if !state.match_active || state.current_round == 0 {
                return Err("请先进入一局酒馆战棋再和 AI 交流".to_owned());
            }
            if state.chat_busy {
                return Err("AI 正在回复上一条消息，请稍等".to_owned());
            }
            if state.last_snapshot.is_none() {
                return Err("当前还没有稳定游戏状态".to_owned());
            }
        }
        let round_for_task = self.inner.read().unwrap().current_round;
        let task = self.start_ai_task("chat", "AI 对话", Some(round_for_task))?;
        let client = self.client_for_task(&config, &task, "chat");

        let (generation, chat_target_round, snapshot, selected, watchlist, round_plan, tactical_plan, trinket_plan, history) = {
            let mut state = self.inner.write().unwrap();
            let mut snapshot = state
                .last_snapshot
                .clone()
                .expect("validated stable snapshot before starting chat task");
            // The UI/current_round is newer than any stale cached snapshot. Chat
            // must never infer the current turn/phase from an older ShopUpdated.
            snapshot.round_number = state.current_round;
            snapshot.phase = state.current_phase.clone();
            if !state.stable_board.is_empty() {
                snapshot.board = state.stable_board.clone();
            }
            let history = state
                .chat_messages
                .iter()
                .rev()
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>();
            state.chat_generation = state.chat_generation.saturating_add(1);
            let generation = state.chat_generation;
            state.chat_busy = true;
            state.chat_status = "AI 正在理解你的想法…".to_owned();
            state.chat_messages.push(ChatMessage {
                role: "user".to_owned(),
                content: message.to_owned(),
                plan_changed: false,
            });
            trim_chat_history(&mut state.chat_messages);
            let round = state.current_round;
            let chat_target_round = state
                .round_plan
                .as_ref()
                .map(|plan| plan.target_round)
                .filter(|target| *target > 0)
                .unwrap_or_else(|| {
                    if snapshot.phase.eq_ignore_ascii_case("Combat") {
                        round.saturating_add(1)
                    } else {
                        round
                    }
                });
            (
                generation,
                chat_target_round,
                snapshot,
                state.selected_composition.clone(),
                state.watchlist.clone(),
                state.round_plan.clone(),
                state.tactical_plan.clone(),
                state.trinket_plan.clone(),
                history,
            )
        };

        let this = self.clone();
        let user_message = message.to_owned();
        thread::spawn(move || {
            task.progress("整理实时上下文", 20, "绑定当前回合、HDT事实和Rust Tactical Plan");
            task.progress("请求模型", 45, "等待 AI 回复；可在 Control Center 随时取消");
            let result = client.chat_with_coach(
                &user_message,
                &snapshot,
                selected.as_ref(),
                watchlist.as_ref(),
                round_plan.as_ref(),
                tactical_plan.as_ref(),
                trinket_plan.as_ref(),
                &history,
                &this.knowledge,
            );

            match result {
                Ok((response, usage)) => {
                    let mut plan_changed = false;
                    let stale_for_patch;
                    {
                        let mut state = this.inner.write().unwrap();
                        if generation != state.chat_generation || !state.match_active {
                            drop(state);
                            task.cancelled("局势已变化，旧对话结果已丢弃");
                            return;
                        }
                        add_usage(&mut state.usage, &usage);

                        // A chat started during Combat is often intended for the
                        // immediately following Recruit. Allow that phase boundary as
                        // long as the live plan still targets the same round; reject only
                        // when the game has moved beyond/replaced that target.
                        let live_target = state
                            .round_plan
                            .as_ref()
                            .map(|plan| plan.target_round)
                            .filter(|target| *target > 0);
                        stale_for_patch = state.current_round > chat_target_round
                            || live_target
                                .map(|target| target != chat_target_round)
                                .unwrap_or(false);
                        if !stale_for_patch {
                            if let Some(patch) = response.plan_patch.as_ref() {
                                let target_round = chat_target_round;
                                if state.round_plan.is_none() {
                                    state.round_plan = Some(
                                        fallback_strategic_plan(&snapshot, target_round, false)
                                            .round_plan,
                                    );
                                }
                                if let Some(plan) = state.round_plan.as_mut() {
                                    plan_changed = patch.apply_to(plan);
                                }
                                if plan_changed {
                                    // User correction has priority over any older slow
                                    // planner request that may still be in flight.
                                    state.planner_generation = state.planner_generation.saturating_add(1);
                                    append_replan_events(
                                        &mut state.replan_events,
                                        &[ReplanEvent::new(
                                            ReplanLevel::Strategic,
                                            "user_chat_plan_update",
                                            if response.patch_summary.trim().is_empty() {
                                                "玩家通过 AI 对话修正了当前小目标".to_owned()
                                            } else {
                                                response.patch_summary.clone()
                                            },
                                        )],
                                    );
                                }
                            }
                        }

                        let cited_facts = response
                            .cited_card_ids
                            .iter()
                            .filter_map(|card_id| {
                                this.catalog.resolve(card_id).map(|meta| {
                                    format!(
                                        "{}({})",
                                        meta.preferred_name().unwrap_or(card_id),
                                        card_id
                                    )
                                })
                            })
                            .take(6)
                            .collect::<Vec<_>>()
                            .join("、");
                        let mut reply = if stale_for_patch && response.plan_patch.is_some() {
                            format!(
                                "{}\n（局势已经进入新阶段，这次对旧目标的修改没有自动写入。）",
                                response.reply
                            )
                        } else {
                            response.reply.clone()
                        };
                        if !cited_facts.is_empty() {
                            reply.push_str(&format!("\n\n[HDT事实依据] {cited_facts}"));
                        }
                        state.chat_messages.push(ChatMessage {
                            role: "assistant".to_owned(),
                            content: reply,
                            plan_changed,
                        });
                        trim_chat_history(&mut state.chat_messages);
                        state.chat_busy = false;
                        state.chat_status = if plan_changed {
                            "目标已修正 · 实时回合/HDT事实校验通过 · 已重算当前决策".to_owned()
                        } else {
                            "AI 已回复 · 实时回合/HDT事实校验通过".to_owned()
                        };
                    }
                    if plan_changed {
                        this.recompute_tactical();
                    }
                    task.complete(if plan_changed { "AI 已回复并修改当前目标" } else { "AI 已回复" });
                    this.trace_event(
                        "chat",
                        if plan_changed { "AI 对话 · 目标已修改" } else { "AI 对话完成" },
                        &response.reply,
                        Some(task.id()),
                    );
                }
                Err(DeepSeekError::Cancelled) => {
                    let mut state = this.inner.write().unwrap();
                    if generation == state.chat_generation {
                        state.chat_busy = false;
                        state.chat_status = "AI 对话已取消".to_owned();
                    }
                    drop(state);
                    task.cancelled("用户取消 AI 对话");
                }
                Err(error) => {
                    let mut state = this.inner.write().unwrap();
                    if generation != state.chat_generation {
                        drop(state);
                        task.cancelled("局势已变化，旧对话错误结果已丢弃");
                        return;
                    }
                    state.chat_busy = false;
                    state.chat_status = "AI 对话失败".to_owned();
                    state.chat_messages.push(ChatMessage {
                        role: "assistant".to_owned(),
                        content: format!("对话请求失败：{error}"),
                        plan_changed: false,
                    });
                    trim_chat_history(&mut state.chat_messages);
                    drop(state);
                    task.failed(&error.to_string());
                }
            }
        });
        Ok(())
    }

    pub fn toggle_overlay_test_mode(&self) -> bool {
        let mut state = self.inner.write().unwrap();
        state.overlay_test_mode = !state.overlay_test_mode;
        state.overlay_test_mode
    }

    pub fn set_overlay_test_mode(&self, enabled: bool) {
        self.inner.write().unwrap().overlay_test_mode = enabled;
    }

    /// Persist a user-adjusted decision-panel rectangle as ratios of the
    /// Hearthstone client area. Position is measured from the client top-left.
    pub fn update_overlay_panel_layout(
        &self,
        x_ratio: f32,
        y_ratio: f32,
        width_ratio: f32,
        height_ratio: f32,
    ) -> Result<(), String> {
        let mut config = self.config.write().unwrap();
        config.overlay.panel_x_ratio = Some(x_ratio.clamp(0.0, 1.0));
        config.overlay.panel_y_ratio = Some(y_ratio.clamp(0.0, 1.0));
        config.overlay.panel_width_ratio = width_ratio.clamp(0.12, 0.60);
        config.overlay.panel_height_ratio = height_ratio.clamp(0.14, 0.70);
        config
            .save(self.config_path.as_ref().as_path())
            .map_err(|error| error.to_string())
    }

    /// Return the decision panel to the compact bottom-right default.
    pub fn reset_overlay_panel_layout(&self) -> Result<(), String> {
        let defaults = OverlayConfig::default();
        let mut config = self.config.write().unwrap();
        config.overlay.panel_x_ratio = None;
        config.overlay.panel_y_ratio = None;
        config.overlay.panel_width_ratio = defaults.panel_width_ratio;
        config.overlay.panel_height_ratio = defaults.panel_height_ratio;
        config.overlay.panel_right_margin_ratio = defaults.panel_right_margin_ratio;
        config.overlay.panel_bottom_margin_ratio = defaults.panel_bottom_margin_ratio;
        config
            .save(self.config_path.as_ref().as_path())
            .map_err(|error| error.to_string())
    }

    pub fn reset_composition_choice(&self) {
        let mut state = self.inner.write().unwrap();
        state.selected_composition = None;
        state.watchlist = None;
        state.shop_hits.clear();
        state.decision_hits.clear();
        state.decision_revision = 0;
        state.ai_status = if state.compositions.is_empty() { "可以让 AI 分析阵容".to_owned() } else { "请选择一个阵容方向".to_owned() };
    }

    fn update_deepseek_config(&self, patch: DeepSeekConfigPatch) -> Result<(), String> {
        let mut config = self.config.write().unwrap();
        if let Some(value) = patch.api_key {
            if !value.trim().is_empty() { config.deepseek.api_key = value; }
        }
        if let Some(value) = patch.base_url { config.deepseek.base_url = value; }
        if let Some(value) = patch.model { config.deepseek.model = value; }
        if let Some(value) = patch.thinking { config.deepseek.thinking = value; }
        config.save(self.config_path.as_ref().as_path()).map_err(|error| error.to_string())
    }
}

#[derive(Debug, Deserialize)]
struct SelectRequest { id: String }

#[derive(Debug, Deserialize)]
struct ChatRequest { message: String }

#[derive(Debug, Deserialize)]
struct TaskCancelRequest { id: u64 }

#[derive(Debug, Deserialize)]
struct DeepSeekConfigPatch {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    thinking: Option<bool>,
}

pub fn run_server(state: DemoState) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = state.config().port;
    let server = Server::http(("127.0.0.1", port))?;
    eprintln!("[demo] control panel: http://127.0.0.1:{port}");
    for request in server.incoming_requests() { handle_request(request, &state); }
    Ok(())
}

fn handle_request(mut request: Request, state: &DemoState) {
    let path = request.url().split('?').next().unwrap_or("/").to_owned();
    let method = request.method().clone();
    let result = match (method, path.as_str()) {
        (Method::Get, "/") => respond_html(request, INDEX_HTML),
        (Method::Get, "/api/state") => respond_json(request, StatusCode(200), &state.public_state()),
        (Method::Get, "/api/tasks") => respond_json(request, StatusCode(200), &state.task_records()),
        (Method::Get, "/api/usage") => respond_json(request, StatusCode(200), &state.usage_summary()),
        (Method::Get, "/api/sessions") => match state.session_summaries() {
            Ok(sessions) => respond_json(request, StatusCode(200), &sessions),
            Err(error) => respond_error(request, StatusCode(500), &error),
        },
        (Method::Post, "/api/session/save") => match state.save_current_session() {
            Ok(path) => respond_json(request, StatusCode(200), &serde_json::json!({"ok":true,"path":path})),
            Err(error) => respond_error(request, StatusCode(500), &error),
        },
        (Method::Post, "/api/tasks/cancel") => {
            let body = read_body(&mut request);
            match body.and_then(|body| serde_json::from_str::<TaskCancelRequest>(&body).map_err(|e| e.to_string())) {
                Ok(data) if state.cancel_task(data.id) => respond_message(request, StatusCode(200), "cancel requested"),
                Ok(_) => respond_error(request, StatusCode(404), "task not running"),
                Err(error) => respond_error(request, StatusCode(400), &error),
            }
        },
        (Method::Post, "/api/analyze") => match state.request_analyze() {
            Ok(()) => respond_message(request, StatusCode(202), "started"),
            Err(error) => respond_error(request, StatusCode(400), &error),
        },
        (Method::Post, "/api/select") => {
            let body = read_body(&mut request);
            match body
                .and_then(|body| serde_json::from_str::<SelectRequest>(&body).map_err(|e| e.to_string()))
                .and_then(|data| state.request_select(&data.id))
            {
                Ok(()) => respond_message(request, StatusCode(202), "started"),
                Err(error) => respond_error(request, StatusCode(400), &error),
            }
        }
        (Method::Post, "/api/chat") => {
            let body = read_body(&mut request);
            match body
                .and_then(|body| serde_json::from_str::<ChatRequest>(&body).map_err(|e| e.to_string()))
                .and_then(|data| state.request_chat(&data.message))
            {
                Ok(()) => respond_message(request, StatusCode(202), "started"),
                Err(error) => respond_error(request, StatusCode(400), &error),
            }
        }
        (Method::Post, "/api/overlay/test") => {
            let enabled = state.toggle_overlay_test_mode();
            respond_json(request, StatusCode(200), &serde_json::json!({"ok": true, "enabled": enabled}))
        }
        (Method::Post, "/api/config") => {
            let body = read_body(&mut request);
            match body
                .and_then(|body| serde_json::from_str::<DeepSeekConfigPatch>(&body).map_err(|e| e.to_string()))
                .and_then(|patch| state.update_deepseek_config(patch))
            {
                Ok(()) => respond_message(request, StatusCode(200), "saved"),
                Err(error) => respond_error(request, StatusCode(400), &error),
            }
        }
        _ => respond_error(request, StatusCode(404), "not found"),
    };
    if let Err(error) = result { eprintln!("[demo] HTTP response error: {error}"); }
}

fn read_body(request: &mut Request) -> Result<String, String> {
    let mut body = String::new();
    request.as_reader().take(64 * 1024).read_to_string(&mut body).map_err(|error| error.to_string())?;
    Ok(body)
}

fn respond_html(request: Request, body: &str) -> std::io::Result<()> {
    let header = Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap();
    request.respond(Response::from_string(body).with_header(header))
}

fn respond_json<T: Serialize>(request: Request, status: StatusCode, value: &T) -> std::io::Result<()> {
    let body = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned());
    let header = Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap();
    request.respond(Response::from_string(body).with_status_code(status).with_header(header))
}

fn respond_message(request: Request, status: StatusCode, message: &str) -> std::io::Result<()> {
    respond_json(request, status, &serde_json::json!({"ok": true, "message": message}))
}

fn respond_error(request: Request, status: StatusCode, message: &str) -> std::io::Result<()> {
    respond_json(request, status, &serde_json::json!({"ok": false, "error": message}))
}

fn recompute_hits_locked(state: &mut MutableState, catalog: &CardCatalog) {
    state.shop_hits.clear();
    let Some(watchlist) = state.watchlist.as_ref() else { return; };
    let current_stage = stage_for_round(state.current_round);
    for (shop_index, card_id) in state.current_shop.iter().enumerate() {
        let normal_id = catalog.resolve(card_id).and_then(|meta| meta.normal_card_id()).unwrap_or(card_id);
        let matched = watchlist
            .find_card_for_stage(current_stage, card_id)
            .or_else(|| watchlist.find_card_for_stage(current_stage, normal_id));
        let Some((stage, watch)) = matched else { continue; };
        let resolved = catalog.resolve(card_id);
        state.shop_hits.push(ShopHit {
            shop_index,
            card_id: card_id.clone(),
            name: resolved.as_ref().and_then(|card| card.preferred_name()).unwrap_or(card_id).to_owned(),
            tavern_tier: resolved.as_ref().and_then(|card| card.tavern_tier()),
            stage: stage.to_owned(),
            priority: watch.priority.clone(),
            role: watch.role.clone(),
            reason: watch.reason.clone(),
            text: resolved.as_ref().and_then(|card| card.preferred_text()).unwrap_or("").to_owned(),
        });
    }
}

fn tactical_hits(plan: &TacticalPlan, snapshot: &AgentSnapshot, catalog: &CardCatalog) -> Vec<ShopHit> {
    plan.ranked_actions
        .iter()
        .filter(|action| action.kind == "buy" && action.score >= 5.0)
        .filter_map(|action| {
            let index = action.shop_index?;
            let card = snapshot.shop.get(index)?;
            let resolved = catalog.resolve(&card.card_id);
            Some(ShopHit {
                shop_index: index,
                card_id: card.card_id.clone(),
                name: card.name.clone(),
                tavern_tier: card.tavern_tier.or_else(|| resolved.as_ref().and_then(|meta| meta.tavern_tier())),
                stage: "dynamic".to_owned(),
                priority: if action.score >= 9.0 { "S" } else if action.score >= 7.0 { "A" } else { "B" }.to_owned(),
                role: "当前决策".to_owned(),
                reason: action.reason.clone(),
                text: card.text.clone(),
            })
        })
        .collect()
}

fn trim_chat_history(messages: &mut Vec<ChatMessage>) {
    const MAX_CHAT_MESSAGES: usize = 20;
    if messages.len() > MAX_CHAT_MESSAGES {
        messages.drain(0..messages.len() - MAX_CHAT_MESSAGES);
    }
}

fn append_replan_events(target: &mut Vec<ReplanEvent>, incoming: &[ReplanEvent]) {
    for event in incoming {
        if !target.iter().rev().take(8).any(|old| old.code == event.code && old.detail == event.detail) {
            target.push(event.clone());
        }
    }
    if target.len() > 12 {
        target.drain(0..target.len() - 12);
    }
}

fn add_usage(total: &mut AiUsage, delta: &AiUsage) {
    total.prompt_tokens = total.prompt_tokens.saturating_add(delta.prompt_tokens);
    total.completion_tokens = total.completion_tokens.saturating_add(delta.completion_tokens);
    total.total_tokens = total.total_tokens.saturating_add(delta.total_tokens);
    total.prompt_cache_hit_tokens = total
        .prompt_cache_hit_tokens
        .saturating_add(delta.prompt_cache_hit_tokens);
    total.prompt_cache_miss_tokens = total
        .prompt_cache_miss_tokens
        .saturating_add(delta.prompt_cache_miss_tokens);
}

fn phase_name(phase: PhaseKind) -> &'static str {
    match phase {
        PhaseKind::Recruit => "Recruit",
        PhaseKind::Combat => "Combat",
        PhaseKind::Complete => "Complete",
        PhaseKind::Pregame => "Pregame",
        PhaseKind::Unknown => "Unknown",
    }
}

fn stage_for_round(round: u32) -> &'static str {
    match round { 0..=4 => "early", 5..=8 => "mid", _ => "late" }
}

fn choice_option_views(choice: &crate::harness::ChoiceArchive) -> Vec<ChoiceOptionView> {
    choice
        .options
        .iter()
        .map(|card| ChoiceOptionView {
            card_id: card
                .card_id
                .clone()
                .unwrap_or_else(|| format!("entity:{}", card.entity_id)),
            name: card
                .name
                .clone()
                .or_else(|| card.card_id.clone())
                .unwrap_or_else(|| "unknown".to_owned()),
            text: card.text.clone().unwrap_or_default(),
        })
        .collect()
}

fn fallback_trinket_rankings(
    options: &[ChoiceOptionView],
    plan: Option<&TrinketPlan>,
) -> Vec<TrinketOptionRecommendation> {
    let top_role = plan
        .and_then(|plan| plan.role_priorities.first())
        .map(|item| item.role.as_str())
        .unwrap_or("按当前阵容判断");
    let desired = plan
        .map(|plan| plan.desired_effect.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("优先选择最契合当前小目标的饰品");
    options
        .iter()
        .map(|option| TrinketOptionRecommendation {
            card_id: option.card_id.clone(),
            name: option.name.clone(),
            score: 5.0,
            role: top_role.to_owned(),
            reason: format!("已识别实际候选；{desired}。AI 排序不可用时请结合卡牌文字比较。"),
        })
        .collect()
}
