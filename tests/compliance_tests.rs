use std::sync::atomic::Ordering;

use hearthcoach_harness::demo::{
    compliance::{calculate_cost, summarize_usage, ApiCallRecord, CostBreakdown, TaskManager, TaskStatus},
    config::{BudgetConfig, PricingConfig},
    model::AiUsage,
};

#[test]
fn exact_usage_cost_uses_provider_usage_fields() {
    let usage = AiUsage {
        prompt_tokens: 1_000_000,
        completion_tokens: 500_000,
        total_tokens: 1_500_000,
        prompt_cache_hit_tokens: 200_000,
        prompt_cache_miss_tokens: 800_000,
    };
    let pricing = PricingConfig {
        currency: "CNY".to_owned(),
        input_per_million: 2.0,
        output_per_million: 8.0,
        cached_input_per_million: 0.5,
    };
    let cost = calculate_cost(&usage, &pricing);
    assert!((cost.uncached_input_cost - 1.6).abs() < 1e-9);
    assert!((cost.cached_input_cost - 0.1).abs() < 1e-9);
    assert!((cost.output_cost - 4.0).abs() < 1e-9);
    assert!((cost.total_cost - 5.7).abs() < 1e-9);
}

#[test]
fn budget_exhaustion_is_detected() {
    let usage = AiUsage {
        prompt_tokens: 800,
        completion_tokens: 300,
        total_tokens: 1_100,
        ..AiUsage::default()
    };
    let call = ApiCallRecord {
        id: 1,
        task_id: Some(1),
        kind: "test".to_owned(),
        model: "m".to_owned(),
        base_url: "local".to_owned(),
        started_at_ms: 1,
        finished_at_ms: 2,
        status: "ok".to_owned(),
        error: None,
        usage,
        cost: CostBreakdown {
            currency: "CNY".to_owned(),
            total_cost: 0.2,
            ..CostBreakdown::default()
        },
    };
    let budget = BudgetConfig {
        max_total_tokens: Some(1_000),
        max_cost: Some(1.0),
    };
    let summary = summarize_usage(&[call], &budget, "CNY");
    assert!(summary.budget_exhausted);
    assert!(summary.token_budget_ratio.unwrap() > 1.0);
}

#[test]
fn task_manager_exposes_real_cancel_flag() {
    let tasks = TaskManager::new(50);
    let task = tasks.start("llm", "long request", Some(3));
    let flag = task.cancel_flag();
    assert!(!flag.load(Ordering::Acquire));
    assert!(tasks.cancel(task.id()));
    assert!(flag.load(Ordering::Acquire));
    let record = tasks.records().into_iter().find(|record| record.id == task.id()).unwrap();
    assert_eq!(record.status, TaskStatus::CancelRequested);
    task.cancelled("done");
    let record = tasks.records().into_iter().find(|record| record.id == task.id()).unwrap();
    assert_eq!(record.status, TaskStatus::Cancelled);
}

#[test]
fn session_archive_roundtrips_full_agent_context() {
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};
    use hearthcoach_harness::demo::{
        compliance::{load_session, save_session, AgentSessionArchive, TraceEvent},
        model::{ChatMessage, RoundPlan},
    };

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("hearthcoach_session_test_{stamp}"));
    let mut archive = AgentSessionArchive::default();
    archive.schema_version = "0.5.0".to_owned();
    archive.session_id = "roundtrip".to_owned();
    archive.round_plan = Some(RoundPlan {
        primary_goal: "本回合优先升本".to_owned(),
        ..RoundPlan::default()
    });
    archive.chat_messages.push(ChatMessage {
        role: "user".to_owned(),
        content: "不要转型".to_owned(),
        plan_changed: false,
    });
    archive.trace.push(TraceEvent {
        category: "planner".to_owned(),
        title: "战略计划".to_owned(),
        detail: "结构化计划快照".to_owned(),
        ..TraceEvent::default()
    });

    let path = save_session(&dir, &archive).unwrap();
    let loaded = load_session(&path).unwrap();
    assert_eq!(loaded.session_id, "roundtrip");
    assert_eq!(loaded.round_plan.unwrap().primary_goal, "本回合优先升本");
    assert_eq!(loaded.chat_messages[0].content, "不要转型");
    assert_eq!(loaded.trace[0].title, "战略计划");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn old_model_config_json_gets_new_course_defaults() {
    use hearthcoach_harness::demo::config::DeepSeekConfig;
    let old = r#"{
        "base_url":"http://127.0.0.1:8000/v1",
        "api_key":"",
        "model":"local-model",
        "thinking":false,
        "max_tokens":2048
    }"#;
    let cfg: DeepSeekConfig = serde_json::from_str(old).unwrap();
    assert!(cfg.context_window >= 1024);
    assert!(cfg.timeout_seconds >= 3);
    assert_eq!(cfg.pricing.input_per_million, 0.0);
    assert!(cfg.budget.max_total_tokens.is_none());
}
