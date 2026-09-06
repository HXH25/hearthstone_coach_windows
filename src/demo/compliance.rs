use std::{
    collections::HashMap,
    fs,
    io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::harness::GameArchive;

use super::{
    config::{BudgetConfig, DeepSeekConfig, PricingConfig},
    model::{
        AiUsage, ChatMessage, CompositionOption, ReplanEvent, RoundPlan, TacticalPlan,
        TrinketPlan, WatchlistResponse,
    },
};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Running,
    Completed,
    CancelRequested,
    Cancelled,
    Failed,
}

impl Default for TaskStatus {
    fn default() -> Self {
        Self::Running
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskRecord {
    pub id: u64,
    pub kind: String,
    pub label: String,
    pub round_number: Option<u32>,
    pub stage: String,
    pub progress_percent: u8,
    pub detail: String,
    pub status: TaskStatus,
    pub started_at_ms: u64,
    pub finished_at_ms: Option<u64>,
}

#[derive(Clone, Default)]
pub struct TaskManager {
    inner: Arc<Mutex<TaskManagerInner>>,
    next_id: Arc<AtomicU64>,
}

#[derive(Default)]
struct TaskManagerInner {
    records: Vec<TaskRecord>,
    cancel_flags: HashMap<u64, Arc<AtomicBool>>,
    history_limit: usize,
}

#[derive(Clone)]
pub struct TaskHandle {
    id: u64,
    manager: TaskManager,
    cancel: Arc<AtomicBool>,
}

impl TaskManager {
    pub fn new(history_limit: usize) -> Self {
        let manager = Self::default();
        manager.inner.lock().unwrap().history_limit = history_limit.max(50);
        manager
    }

    pub fn start(
        &self,
        kind: impl Into<String>,
        label: impl Into<String>,
        round_number: Option<u32>,
    ) -> TaskHandle {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).saturating_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let record = TaskRecord {
            id,
            kind: kind.into(),
            label: label.into(),
            round_number,
            stage: "准备".to_owned(),
            progress_percent: 0,
            detail: String::new(),
            status: TaskStatus::Running,
            started_at_ms: now_ms(),
            finished_at_ms: None,
        };
        let mut inner = self.inner.lock().unwrap();
        inner.records.push(record);
        inner.cancel_flags.insert(id, cancel.clone());
        trim_tasks(&mut inner);
        TaskHandle {
            id,
            manager: self.clone(),
            cancel,
        }
    }

    pub fn records(&self) -> Vec<TaskRecord> {
        self.inner.lock().unwrap().records.clone()
    }

    pub fn running(&self) -> Vec<TaskRecord> {
        self.inner
            .lock()
            .unwrap()
            .records
            .iter()
            .filter(|record| {
                matches!(record.status, TaskStatus::Running | TaskStatus::CancelRequested)
            })
            .cloned()
            .collect()
    }

    pub fn cancel(&self, id: u64) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let Some(flag) = inner.cancel_flags.get(&id).cloned() else {
            return false;
        };
        flag.store(true, Ordering::Release);
        if let Some(record) = inner.records.iter_mut().find(|record| record.id == id) {
            if record.status == TaskStatus::Running {
                record.status = TaskStatus::CancelRequested;
                record.stage = "正在取消".to_owned();
                record.detail = "已收到用户取消请求，正在终止当前 API 请求".to_owned();
            }
        }
        true
    }

    pub fn cancel_all_running(&self, reason: &str) {
        let ids = self
            .inner
            .lock()
            .unwrap()
            .records
            .iter()
            .filter(|record| record.status == TaskStatus::Running)
            .map(|record| record.id)
            .collect::<Vec<_>>();
        for id in ids {
            if self.cancel(id) {
                self.update(id, "正在取消", 100, reason);
            }
        }
    }

    /// Wait briefly for cancellable worker threads to acknowledge cancellation.
    /// Used at match shutdown so the persisted session contains final task/API
    /// statuses instead of a stale in-flight snapshot.
    pub fn wait_for_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.running().is_empty() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn update(&self, id: u64, stage: &str, progress: u8, detail: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(record) = inner.records.iter_mut().find(|record| record.id == id) {
            if matches!(record.status, TaskStatus::Completed | TaskStatus::Cancelled | TaskStatus::Failed) {
                return;
            }
            record.stage = stage.to_owned();
            record.progress_percent = progress.min(100);
            record.detail = detail.to_owned();
        }
    }

    fn finish(&self, id: u64, status: TaskStatus, detail: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(record) = inner.records.iter_mut().find(|record| record.id == id) {
            record.status = status;
            record.progress_percent = 100;
            record.stage = match status {
                TaskStatus::Completed => "完成",
                TaskStatus::Cancelled => "已取消",
                TaskStatus::Failed => "失败",
                TaskStatus::CancelRequested => "正在取消",
                TaskStatus::Running => "运行中",
            }
            .to_owned();
            record.detail = detail.to_owned();
            record.finished_at_ms = Some(now_ms());
        }
        inner.cancel_flags.remove(&id);
        trim_tasks(&mut inner);
    }
}

fn trim_tasks(inner: &mut TaskManagerInner) {
    let limit = inner.history_limit.max(50);
    if inner.records.len() <= limit {
        return;
    }
    // Never drop currently running tasks. Remove the oldest finished records.
    let overflow = inner.records.len() - limit;
    let mut remove = 0usize;
    while remove < overflow {
        let Some(index) = inner.records.iter().position(|record| {
            !matches!(record.status, TaskStatus::Running | TaskStatus::CancelRequested)
        }) else {
            break;
        };
        inner.records.remove(index);
        remove += 1;
    }
}

impl TaskHandle {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    pub fn progress(&self, stage: &str, progress_percent: u8, detail: &str) {
        self.manager
            .update(self.id, stage, progress_percent, detail);
    }

    pub fn complete(&self, detail: &str) {
        self.manager.finish(self.id, TaskStatus::Completed, detail);
    }

    pub fn cancelled(&self, detail: &str) {
        self.manager.finish(self.id, TaskStatus::Cancelled, detail);
    }

    pub fn failed(&self, detail: &str) {
        self.manager.finish(self.id, TaskStatus::Failed, detail);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostBreakdown {
    pub currency: String,
    pub uncached_input_cost: f64,
    pub cached_input_cost: f64,
    pub output_cost: f64,
    pub total_cost: f64,
}

pub fn calculate_cost(usage: &AiUsage, pricing: &PricingConfig) -> CostBreakdown {
    let cached = usage.prompt_cache_hit_tokens.min(usage.prompt_tokens);
    let uncached = usage.prompt_tokens.saturating_sub(cached);
    let uncached_input_cost = uncached as f64 / 1_000_000.0 * pricing.input_per_million.max(0.0);
    let cached_rate = if pricing.cached_input_per_million > 0.0 {
        pricing.cached_input_per_million
    } else {
        pricing.input_per_million.max(0.0)
    };
    let cached_input_cost = cached as f64 / 1_000_000.0 * cached_rate;
    let output_cost =
        usage.completion_tokens as f64 / 1_000_000.0 * pricing.output_per_million.max(0.0);
    CostBreakdown {
        currency: pricing.currency.clone(),
        uncached_input_cost,
        cached_input_cost,
        output_cost,
        total_cost: uncached_input_cost + cached_input_cost + output_cost,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApiCallRecord {
    pub id: u64,
    pub task_id: Option<u64>,
    pub kind: String,
    pub model: String,
    pub base_url: String,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
    pub status: String,
    pub error: Option<String>,
    pub usage: AiUsage,
    pub cost: CostBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TraceEvent {
    pub timestamp_ms: u64,
    pub round_number: Option<u32>,
    pub phase: Option<String>,
    pub category: String,
    pub title: String,
    pub detail: String,
    pub task_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelConfigSnapshot {
    pub api_compatibility: String,
    pub base_url: String,
    pub model: String,
    pub thinking: bool,
    pub max_tokens: u32,
    pub context_window: u32,
    pub timeout_seconds: u64,
    /// We never persist the secret itself; only whether one was configured.
    pub api_key_configured: bool,
}

impl From<&DeepSeekConfig> for ModelConfigSnapshot {
    fn from(config: &DeepSeekConfig) -> Self {
        Self {
            api_compatibility: config.api_compatibility.clone(),
            base_url: config.base_url.clone(),
            model: config.model.clone(),
            thinking: config.thinking,
            max_tokens: config.max_tokens,
            context_window: config.context_window,
            timeout_seconds: config.timeout_seconds,
            api_key_configured: !config.api_key.trim().is_empty(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageSummary {
    pub usage: AiUsage,
    pub cost: CostBreakdown,
    pub api_call_count: usize,
    pub budget_exhausted: bool,
    pub token_budget: Option<u64>,
    pub cost_budget: Option<f64>,
    pub token_budget_ratio: Option<f64>,
    pub cost_budget_ratio: Option<f64>,
}

pub fn summarize_usage(
    calls: &[ApiCallRecord],
    budget: &BudgetConfig,
    currency: &str,
) -> UsageSummary {
    let mut usage = AiUsage::default();
    let mut total_cost = 0.0;
    let mut uncached_input_cost = 0.0;
    let mut cached_input_cost = 0.0;
    let mut output_cost = 0.0;
    for call in calls {
        usage.prompt_tokens = usage.prompt_tokens.saturating_add(call.usage.prompt_tokens);
        usage.completion_tokens = usage
            .completion_tokens
            .saturating_add(call.usage.completion_tokens);
        usage.total_tokens = usage.total_tokens.saturating_add(call.usage.total_tokens);
        usage.prompt_cache_hit_tokens = usage
            .prompt_cache_hit_tokens
            .saturating_add(call.usage.prompt_cache_hit_tokens);
        usage.prompt_cache_miss_tokens = usage
            .prompt_cache_miss_tokens
            .saturating_add(call.usage.prompt_cache_miss_tokens);
        total_cost += call.cost.total_cost;
        uncached_input_cost += call.cost.uncached_input_cost;
        cached_input_cost += call.cost.cached_input_cost;
        output_cost += call.cost.output_cost;
    }
    let token_budget = budget.max_total_tokens.filter(|value| *value > 0);
    let cost_budget = budget.max_cost.filter(|value| *value > 0.0);
    let token_budget_ratio = token_budget.map(|max| usage.total_tokens as f64 / max as f64);
    let cost_budget_ratio = cost_budget.map(|max| total_cost / max);
    let budget_exhausted = token_budget_ratio.map(|ratio| ratio >= 1.0).unwrap_or(false)
        || cost_budget_ratio.map(|ratio| ratio >= 1.0).unwrap_or(false);
    UsageSummary {
        usage,
        cost: CostBreakdown {
            currency: currency.to_owned(),
            uncached_input_cost,
            cached_input_cost,
            output_cost,
            total_cost,
        },
        api_call_count: calls.len(),
        budget_exhausted,
        token_budget,
        cost_budget,
        token_budget_ratio,
        cost_budget_ratio,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentSessionArchive {
    pub schema_version: String,
    pub session_id: String,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
    pub saved_at_ms: u64,
    pub model: ModelConfigSnapshot,
    pub pricing: PricingConfig,
    pub budget: BudgetConfig,
    pub game_archive: Option<GameArchive>,
    pub available_tribes: Vec<String>,
    pub selected_composition: Option<CompositionOption>,
    pub watchlist: Option<WatchlistResponse>,
    pub round_plan: Option<RoundPlan>,
    pub trinket_plan: Option<TrinketPlan>,
    pub tactical_plan: Option<TacticalPlan>,
    pub replan_events: Vec<ReplanEvent>,
    pub chat_messages: Vec<ChatMessage>,
    pub api_calls: Vec<ApiCallRecord>,
    pub tasks: Vec<TaskRecord>,
    pub trace: Vec<TraceEvent>,
    pub usage: UsageSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionSummary {
    pub session_id: String,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
    pub final_place: Option<u8>,
    pub rounds: usize,
    pub selected_composition: Option<String>,
    pub total_tokens: u64,
    pub total_cost: f64,
    pub currency: String,
    pub path: PathBuf,
}

pub fn new_session_id() -> String {
    format!("session_{}", now_ms())
}

pub fn save_session(history_dir: &Path, archive: &AgentSessionArchive) -> io::Result<PathBuf> {
    fs::create_dir_all(history_dir)?;
    let path = history_dir.join(format!("{}.json", archive.session_id));
    let text = serde_json::to_string_pretty(archive)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    fs::write(&path, text)?;
    Ok(path)
}

pub fn load_session(path: &Path) -> io::Result<AgentSessionArchive> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub fn list_sessions(history_dir: &Path) -> io::Result<Vec<SessionSummary>> {
    if !history_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut summaries = Vec::new();
    for entry in fs::read_dir(history_dir)? {
        let Ok(entry) = entry else { continue; };
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(archive) = load_session(&path) else { continue; };
        let final_place = archive.game_archive.as_ref().and_then(|game| game.final_place);
        let rounds = archive
            .game_archive
            .as_ref()
            .map(|game| game.rounds.len())
            .unwrap_or(0);
        summaries.push(SessionSummary {
            session_id: archive.session_id.clone(),
            started_at_ms: archive.started_at_ms,
            ended_at_ms: archive.ended_at_ms,
            final_place,
            rounds,
            selected_composition: archive.selected_composition.as_ref().map(|item| item.name.clone()),
            total_tokens: archive.usage.usage.total_tokens,
            total_cost: archive.usage.cost.total_cost,
            currency: archive.usage.cost.currency.clone(),
            path,
        });
    }
    summaries.sort_by_key(|summary| std::cmp::Reverse(summary.started_at_ms));
    Ok(summaries)
}
