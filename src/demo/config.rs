use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemoConfig {
    #[serde(default = "default_hearthstone_dir")]
    pub hearthstone_dir: PathBuf,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub open_browser_on_start: bool,
    #[serde(default)]
    pub deepseek: DeepSeekConfig,
    #[serde(default)]
    pub overlay: OverlayConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub compliance: ComplianceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepSeekConfig {
    /// `deepseek` keeps DeepSeek-specific thinking payloads; `openai` omits them
    /// so local vLLM/LM Studio/OpenRouter-style endpoints can be used.
    #[serde(default = "default_api_compatibility")]
    pub api_compatibility: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub thinking: bool,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Model context capacity. This is not sent as an API parameter; it is used
    /// by the Agent when truncating/history budgeting and is exposed in the UI.
    #[serde(default = "default_context_window")]
    pub context_window: u32,
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub pricing: PricingConfig,
    #[serde(default)]
    pub budget: BudgetConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingConfig {
    #[serde(default = "default_currency")]
    pub currency: String,
    /// Price per 1,000,000 uncached input tokens.
    #[serde(default)]
    pub input_per_million: f64,
    /// Price per 1,000,000 output tokens.
    #[serde(default)]
    pub output_per_million: f64,
    /// Optional lower price for provider-reported cache-hit input tokens.
    #[serde(default)]
    pub cached_input_per_million: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetConfig {
    /// `None` or 0 means unlimited. Enforced at API-call boundaries.
    #[serde(default)]
    pub max_total_tokens: Option<u64>,
    /// `None` or <=0 means unlimited. Currency follows PricingConfig.currency.
    #[serde(default)]
    pub max_cost: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceConfig {
    #[serde(default = "default_history_dir")]
    pub history_dir: PathBuf,
    #[serde(default = "default_true")]
    pub auto_save_sessions: bool,
    #[serde(default = "default_task_history_limit")]
    pub task_history_limit: usize,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub auto_plan_during_combat: bool,
    /// Automatically prepare a composition guide once the live Battlegrounds
    /// tribe set becomes available. The first validated composition returned by
    /// the model is selected and its stage watchlist is generated automatically.
    /// Disable this to keep the older manual "analyze -> choose composition" flow.
    #[serde(default = "default_true")]
    pub auto_prepare_guide: bool,
    /// Forecast turns used to pre-plan one Combat earlier. Standard play uses
    /// 6/9, but this is configurable because anomalies/heroes can move offers;
    /// live ChoiceKind::Trinket remains authoritative.
    #[serde(default = "default_trinket_rounds")]
    pub trinket_rounds: Vec<u32>,
    #[serde(default = "default_critical_health")]
    pub critical_health: i32,
    #[serde(default = "default_emergency_health")]
    pub emergency_health: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    // Shop-card highlight layout. The row is centered and spreads according to
    // the live number of shop entities. These values are intentionally kept in
    // the local config so the user can calibrate one machine without touching
    // Rust code.
    #[serde(default = "default_shop_center_x")]
    pub shop_center_x_ratio: f32,
    #[serde(default = "default_shop_top")]
    pub shop_top_ratio: f32,
    /// Fixed horizontal distance between adjacent shop card centers, relative
    /// to Hearthstone client width. A fixed slot pitch matches the actual BG
    /// layout much better than stretching N cards across one total span.
    #[serde(default = "default_shop_slot_spacing")]
    pub shop_slot_spacing_ratio: f32,
    #[serde(default = "default_card_width")]
    pub card_width_ratio: f32,
    #[serde(default = "default_card_height")]
    pub card_height_ratio: f32,
    #[serde(default = "default_border_px")]
    pub border_px: i32,
    /// Keep AI borders hidden briefly after a semantic ShopRevision so the
    /// Hearthstone card animation can settle. This also prevents "pre-boxing".
    #[serde(default = "default_highlight_delay_ms")]
    pub highlight_delay_ms: u64,

    // Interactive in-game decision panel. `panel_x_ratio` / `panel_y_ratio`
    // are populated after the user drags the panel. When absent, the legacy
    // bottom-right anchor is used so old configs migrate without a jump.
    #[serde(default = "default_panel_width")]
    pub panel_width_ratio: f32,
    #[serde(default = "default_panel_height")]
    pub panel_height_ratio: f32,
    #[serde(default)]
    pub panel_x_ratio: Option<f32>,
    #[serde(default)]
    pub panel_y_ratio: Option<f32>,
    #[serde(default = "default_panel_right")]
    pub panel_right_margin_ratio: f32,
    #[serde(default = "default_panel_bottom")]
    pub panel_bottom_margin_ratio: f32,
    #[serde(default = "default_panel_alpha")]
    pub panel_alpha: u8,

    // Horizontal composition guide across the top of the Hearthstone client.
    #[serde(default = "default_guide_width")]
    pub guide_width_ratio: f32,
    #[serde(default = "default_guide_height")]
    pub guide_height_ratio: f32,
    #[serde(default = "default_guide_top")]
    pub guide_top_margin_ratio: f32,
    #[serde(default = "default_guide_alpha")]
    pub guide_alpha: u8,
    #[serde(default = "default_guide_card_limit")]
    pub guide_card_limit: usize,
    /// Optional extra directories containing CardId-named PNG/JPG/WebP art.
    /// HDT common cache roots are searched automatically before these fallbacks.
    #[serde(default)]
    pub card_art_dirs: Vec<PathBuf>,
}

impl Default for DemoConfig {
    fn default() -> Self {
        Self {
            hearthstone_dir: default_hearthstone_dir(),
            port: default_port(),
            open_browser_on_start: false,
            deepseek: DeepSeekConfig::default(),
            overlay: OverlayConfig::default(),
            agent: AgentConfig::default(),
            compliance: ComplianceConfig::default(),
        }
    }
}

impl Default for DeepSeekConfig {
    fn default() -> Self {
        Self {
            api_compatibility: default_api_compatibility(),
            base_url: default_base_url(),
            api_key: String::new(),
            model: default_model(),
            thinking: false,
            max_tokens: default_max_tokens(),
            context_window: default_context_window(),
            timeout_seconds: default_timeout_seconds(),
            pricing: PricingConfig::default(),
            budget: BudgetConfig::default(),
        }
    }
}

impl Default for PricingConfig {
    fn default() -> Self {
        Self {
            currency: default_currency(),
            input_per_million: 0.0,
            output_per_million: 0.0,
            cached_input_per_million: 0.0,
        }
    }
}

impl Default for ComplianceConfig {
    fn default() -> Self {
        Self {
            history_dir: default_history_dir(),
            auto_save_sessions: true,
            task_history_limit: default_task_history_limit(),
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_plan_during_combat: true,
            auto_prepare_guide: true,
            trinket_rounds: default_trinket_rounds(),
            critical_health: default_critical_health(),
            emergency_health: default_emergency_health(),
        }
    }
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            shop_center_x_ratio: default_shop_center_x(),
            shop_top_ratio: default_shop_top(),
            shop_slot_spacing_ratio: default_shop_slot_spacing(),
            card_width_ratio: default_card_width(),
            card_height_ratio: default_card_height(),
            border_px: default_border_px(),
            highlight_delay_ms: default_highlight_delay_ms(),
            panel_width_ratio: default_panel_width(),
            panel_height_ratio: default_panel_height(),
            panel_x_ratio: None,
            panel_y_ratio: None,
            panel_right_margin_ratio: default_panel_right(),
            panel_bottom_margin_ratio: default_panel_bottom(),
            panel_alpha: default_panel_alpha(),
            guide_width_ratio: default_guide_width(),
            guide_height_ratio: default_guide_height(),
            guide_top_margin_ratio: default_guide_top(),
            guide_alpha: default_guide_alpha(),
            guide_card_limit: default_guide_card_limit(),
            card_art_dirs: Vec::new(),
        }
    }
}

impl DemoConfig {
    pub fn config_path() -> PathBuf {
        env::var_os("HEARTHCOACH_DEMO_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("hearthcoach_demo.json"))
    }

    pub fn load_or_create(path: &Path) -> io::Result<Self> {
        if !path.exists() {
            let config = Self::default();
            config.save(path)?;
            return Ok(config);
        }
        let mut bytes = fs::read(path)?;
        if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            bytes.drain(..3);
        }
        serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        fs::write(path, text)
    }
}

fn default_hearthstone_dir() -> PathBuf {
    PathBuf::new()
}
fn default_port() -> u16 {
    8765
}
fn default_base_url() -> String {
    "https://api.deepseek.com".to_owned()
}
fn default_model() -> String {
    "deepseek-v4-flash".to_owned()
}
fn default_max_tokens() -> u32 {
    4096
}

fn default_api_compatibility() -> String {
    "deepseek".to_owned()
}
fn default_context_window() -> u32 {
    64_000
}
fn default_timeout_seconds() -> u64 {
    120
}
fn default_currency() -> String {
    "CNY".to_owned()
}
fn default_history_dir() -> PathBuf {
    PathBuf::from("hearthcoach_sessions")
}
fn default_task_history_limit() -> usize {
    200
}

fn default_true() -> bool {
    true
}
fn default_shop_center_x() -> f32 {
    0.505
}
fn default_shop_top() -> f32 {
    0.285
}
fn default_shop_slot_spacing() -> f32 {
    0.079
}
fn default_card_width() -> f32 {
    0.095
}
fn default_card_height() -> f32 {
    0.215
}
fn default_border_px() -> i32 {
    4
}
fn default_highlight_delay_ms() -> u64 {
    320
}
fn default_panel_width() -> f32 {
    0.235
}
fn default_panel_height() -> f32 {
    0.30
}
fn default_panel_right() -> f32 {
    0.012
}
fn default_panel_bottom() -> f32 {
    0.055
}
fn default_panel_alpha() -> u8 {
    248
}
fn default_guide_width() -> f32 {
    0.76
}
fn default_guide_height() -> f32 {
    0.14
}
fn default_guide_top() -> f32 {
    0.012
}
fn default_guide_alpha() -> u8 {
    246
}
fn default_guide_card_limit() -> usize {
    7
}

fn default_trinket_rounds() -> Vec<u32> { vec![6, 9] }
fn default_critical_health() -> i32 { 15 }
fn default_emergency_health() -> i32 { 8 }
