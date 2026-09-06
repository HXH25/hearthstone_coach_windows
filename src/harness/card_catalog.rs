use std::{
    collections::{BTreeSet, HashMap},
    env,
    error::Error,
    fmt,
    fs,
    io,
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};

use serde::{Deserialize, Serialize};

const CACHE_FILE_NAME: &str = "hearthdb_cards_v4_zhCN.json";
const EXPORT_SCRIPT: &str = include_str!("../../tools/export_hearthdb.ps1");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CardMeta {
    pub card_id: String,
    #[serde(default)]
    pub dbf_id: Option<i32>,
    #[serde(default)]
    pub name_zh_cn: Option<String>,
    #[serde(default)]
    pub name_en_us: Option<String>,
    #[serde(default)]
    pub text_zh_cn: Option<String>,
    #[serde(default)]
    pub text_en_us: Option<String>,
    #[serde(default)]
    pub mechanics: Vec<String>,
    /// Static Battlegrounds Activate keyword from CardDefs, not live usability.
    #[serde(default)]
    pub activate_keyword: bool,
    /// True only when this normal minion is present in HearthDb.Cards.BaconPoolMinions
    /// after loading HDT's newest CardDefs.base.xml. This is the authoritative
    /// current Battlegrounds minion-pool flag used by the demo knowledge layer.
    #[serde(default)]
    pub in_bacon_pool: bool,
    #[serde(default)]
    pub card_type: Option<String>,
    #[serde(default)]
    pub race: Option<String>,
    #[serde(default)]
    pub secondary_race: Option<String>,
    #[serde(default)]
    pub tavern_tier: Option<u8>,
    #[serde(default)]
    pub cost: Option<i32>,
    #[serde(default)]
    pub attack: Option<i32>,
    #[serde(default)]
    pub health: Option<i32>,
    #[serde(default)]
    pub premium: bool,
    #[serde(default)]
    pub normal_card_id: Option<String>,
}

impl CardMeta {
    pub fn preferred_name(&self) -> Option<&str> {
        self.name_zh_cn
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| self.name_en_us.as_deref().filter(|s| !s.trim().is_empty()))
    }

    pub fn preferred_text(&self) -> Option<&str> {
        self.text_zh_cn
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| self.text_en_us.as_deref().filter(|s| !s.trim().is_empty()))
    }
}

#[derive(Debug, Clone, Default)]
pub struct CardCatalog {
    cards: HashMap<String, CardMeta>,
    source: Option<String>,
}

#[derive(Debug)]
pub enum CardCatalogError {
    Io(io::Error),
    Json(serde_json::Error),
    HearthDbNotFound,
    PowerShellFailed(String),
    UnsupportedPlatform,
}

impl fmt::Display for CardCatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Json(error) => write!(f, "card catalog JSON error: {error}"),
            Self::HearthDbNotFound => write!(
                f,
                "could not find HDT's HearthDb.dll; run HearthCoach Launcher.cmd to install/detect HDT automatically, or pass --hearthdb-dll / set HEARTHCOACH_HEARTHDB_DLL"
            ),
            Self::PowerShellFailed(message) => write!(f, "HearthDb export failed: {message}"),
            Self::UnsupportedPlatform => write!(f, "automatic HDT HearthDb export is only supported on Windows"),
        }
    }
}

impl Error for CardCatalogError {}

impl From<io::Error> for CardCatalogError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for CardCatalogError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl CardCatalog {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_cards(cards: impl IntoIterator<Item = CardMeta>) -> Self {
        let mut catalog = Self::default();
        for card in cards {
            catalog.cards.insert(card.card_id.clone(), card);
        }
        catalog.source = Some("in-memory".to_owned());
        catalog
    }

    pub fn len(&self) -> usize {
        self.cards.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub fn get(&self, card_id: &str) -> Option<&CardMeta> {
        self.cards.get(card_id)
    }

    /// Iterate the locally loaded HearthDb snapshot. Higher-level agents may
    /// use this to build compact, auditable prompts without reading HearthDb
    /// or Power.log directly.
    pub fn cards(&self) -> impl Iterator<Item = &CardMeta> {
        self.cards.values()
    }

    /// Resolve static metadata, falling back from a premium Battlegrounds card
    /// to its normal counterpart when HearthDb exposes the triple mapping.
    pub fn resolve(&self, card_id: &str) -> Option<ResolvedCardMeta<'_>> {
        let direct = self.get(card_id);
        let normal_id = direct
            .and_then(|card| card.normal_card_id.as_deref())
            .or_else(|| card_id.strip_suffix("_G"));
        let normal = normal_id.and_then(|id| self.get(id));
        if direct.is_none() && normal.is_none() {
            return None;
        }
        Some(ResolvedCardMeta {
            direct,
            normal,
            premium_fallback: direct.is_none() && card_id.ends_with("_G"),
        })
    }

    pub fn load_json(path: &Path) -> Result<Self, CardCatalogError> {
        let mut bytes = fs::read(path)?;
        if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            bytes.drain(..3);
        }
        let cards: Vec<CardMeta> = serde_json::from_slice(&bytes)?;
        let mut catalog = Self::from_cards(cards);
        catalog.source = Some(path.display().to_string());
        Ok(catalog)
    }

    /// Load a previously exported catalog without invoking PowerShell.
    /// This is suitable for library callers that do not want process spawning.
    pub fn load_cached_default() -> Result<Self, CardCatalogError> {
        if let Ok(path) = env::var("HEARTHCOACH_CARD_DB") {
            return Self::load_json(Path::new(&path));
        }
        Self::load_json(&default_cache_path())
    }

    /// CLI-oriented helper: use HDT's installed HearthDb.dll as the source of
    /// truth. The DLL is queried once through Windows PowerShell and a compact
    /// JSON cache is produced for fast Rust-side HashMap lookups thereafter.
    pub fn load_or_export_hdt(
        hearthdb_override: Option<&Path>,
        force_rebuild: bool,
    ) -> Result<Self, CardCatalogError> {
        if let Ok(path) = env::var("HEARTHCOACH_CARD_DB") {
            return Self::load_json(Path::new(&path));
        }

        let cache = default_cache_path();
        let dll = locate_hearthdb_dll(hearthdb_override)?;
        let carddefs = locate_hdt_carddefs_base();
        let should_export = force_rebuild
            || cache_is_older_than_any(&cache, std::iter::once(dll.as_path()).chain(carddefs.as_deref()))?;

        if should_export {
            export_hdt_catalog(&dll, carddefs.as_deref(), &cache)?;
        }

        let mut catalog = Self::load_json(&cache)?;
        catalog.source = Some(match carddefs {
            Some(carddefs) => format!(
                "{} (HDT HearthDb.dll={}, CardDefs={})",
                cache.display(),
                dll.display(),
                carddefs.display()
            ),
            None => format!("{} (HDT HearthDb.dll={})", cache.display(), dll.display()),
        });
        Ok(catalog)
    }

    pub fn default_cache_path() -> PathBuf {
        default_cache_path()
    }

    pub fn locate_hdt_hearthdb_dll(override_path: Option<&Path>) -> Result<PathBuf, CardCatalogError> {
        locate_hearthdb_dll(override_path)
    }

    /// Locate HDT's downloaded CardDefs.base.xml when present. HDT itself
    /// refreshes this file from HearthstoneJSON; using it keeps the Rust cache
    /// current even when HearthDb.dll's bundled snapshot is older.
    pub fn locate_hdt_carddefs_base() -> Option<PathBuf> {
        locate_hdt_carddefs_base()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ResolvedCardMeta<'a> {
    direct: Option<&'a CardMeta>,
    normal: Option<&'a CardMeta>,
    premium_fallback: bool,
}

impl<'a> ResolvedCardMeta<'a> {
    pub fn card_id(&self) -> &str {
        self.direct
            .map(|card| card.card_id.as_str())
            .or_else(|| self.normal.map(|card| card.card_id.as_str()))
            .unwrap_or("")
    }

    pub fn preferred_name(&self) -> Option<&'a str> {
        self.direct
            .and_then(CardMeta::preferred_name)
            .or_else(|| self.normal.and_then(CardMeta::preferred_name))
    }

    pub fn preferred_text(&self) -> Option<&'a str> {
        self.direct
            .and_then(CardMeta::preferred_text)
            .or_else(|| self.normal.and_then(CardMeta::preferred_text))
    }

    pub fn mechanics(&self) -> Vec<String> {
        let mut result = BTreeSet::new();
        if let Some(direct) = self.direct {
            result.extend(direct.mechanics.iter().cloned());
        }
        if result.is_empty() {
            if let Some(normal) = self.normal {
                result.extend(normal.mechanics.iter().cloned());
            }
        }
        result.into_iter().collect()
    }

    /// Static CardDefs keyword. This says the card has an Activate mechanic;
    /// it does not say the live entity can activate right now.
    pub fn activate_keyword(&self) -> bool {
        self.direct.map(|c| c.activate_keyword).unwrap_or(false)
            || self.normal.map(|c| c.activate_keyword).unwrap_or(false)
    }

    /// Authoritative HDT/HearthDb current Battlegrounds minion-pool membership.
    /// Premium cards inherit the normal card's pool membership.
    pub fn in_bacon_pool(&self) -> bool {
        self.direct.map(|c| c.in_bacon_pool).unwrap_or(false)
            || self.normal.map(|c| c.in_bacon_pool).unwrap_or(false)
    }

    pub fn card_type(&self) -> Option<&'a str> {
        self.direct
            .and_then(|c| c.card_type.as_deref())
            .filter(|s| !is_invalid_symbol(s))
            .or_else(|| {
                self.normal
                    .and_then(|c| c.card_type.as_deref())
                    .filter(|s| !is_invalid_symbol(s))
            })
    }

    pub fn tavern_tier(&self) -> Option<u8> {
        self.direct
            .and_then(|c| nonzero_u8(c.tavern_tier))
            .or_else(|| self.normal.and_then(|c| nonzero_u8(c.tavern_tier)))
    }

    pub fn cost(&self) -> Option<i32> {
        self.direct
            .and_then(|c| c.cost)
            .or_else(|| self.normal.and_then(|c| c.cost))
    }

    pub fn attack(&self) -> Option<i32> {
        self.direct
            .and_then(|c| c.attack)
            .or_else(|| self.normal.and_then(|c| c.attack))
    }

    pub fn health(&self) -> Option<i32> {
        self.direct
            .and_then(|c| c.health)
            .or_else(|| self.normal.and_then(|c| c.health))
    }

    pub fn premium(&self) -> bool {
        self.direct.map(|c| c.premium).unwrap_or(false)
            || self.normal_card_id().is_some()
            || self.premium_fallback
    }

    pub fn normal_card_id(&self) -> Option<&'a str> {
        self.direct
            .and_then(|c| c.normal_card_id.as_deref())
            .or_else(|| self.normal.map(|c| c.card_id.as_str()))
    }

    pub fn tribes(&self) -> Vec<String> {
        let mut result = BTreeSet::new();
        if let Some(direct) = self.direct {
            add_race(&mut result, direct.race.as_deref());
            add_race(&mut result, direct.secondary_race.as_deref());
        }
        if result.is_empty() {
            if let Some(normal) = self.normal {
                add_race(&mut result, normal.race.as_deref());
                add_race(&mut result, normal.secondary_race.as_deref());
            }
        }
        result.into_iter().collect()
    }
}

pub fn normalize_tribe(raw: &str) -> Option<String> {
    // Keep the uppercase String alive for the entire match. Calling
    // `to_ascii_uppercase().as_str()` inline would borrow from a temporary
    // String that is dropped at the end of the statement (E0716).
    let upper = raw.trim().to_ascii_uppercase();
    let normalized = match upper.as_str() {
        "" | "INVALID" | "ALL" | "0" | "26" => return None,
        // HearthMirror has returned numeric HearthDb Race enum values in some
        // versions. Accept them here as a second line of defense even though
        // the PowerShell bridge also converts them to enum names.
        "17" | "MECHANICAL" | "MECH" => "MECH",
        "18" | "ELEMENTAL" | "ELEMENTALS" => "ELEMENTALS",
        "43" | "QUILBOAR" | "QUILLBOAR" => "QUILLBOAR",
        "23" | "PIRATE" => "PIRATE",
        "24" | "DRAGON" => "DRAGON",
        "15" | "DEMON" => "DEMON",
        "14" | "MURLOC" => "MURLOC",
        "92" | "NAGA" => "NAGA",
        "11" | "UNDEAD" => "UNDEAD",
        "20" | "BEAST" => "BEAST",
        other => other,
    };
    Some(normalized.to_owned())
}

fn add_race(set: &mut BTreeSet<String>, race: Option<&str>) {
    if let Some(race) = race.and_then(normalize_tribe) {
        set.insert(race);
    }
}

fn nonzero_u8(value: Option<u8>) -> Option<u8> {
    value.filter(|value| *value > 0)
}

fn is_invalid_symbol(value: &str) -> bool {
    matches!(value.trim().to_ascii_uppercase().as_str(), "" | "INVALID")
}

fn default_cache_path() -> PathBuf {
    if let Ok(path) = env::var("HEARTHCOACH_CARD_DB") {
        return PathBuf::from(path);
    }
    if let Ok(local) = env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("HearthCoach").join(CACHE_FILE_NAME);
    }
    PathBuf::from("data").join(CACHE_FILE_NAME)
}

fn cache_is_older_than_any<'a>(
    cache: &Path,
    sources: impl Iterator<Item = &'a Path>,
) -> Result<bool, CardCatalogError> {
    if !cache.is_file() {
        return Ok(true);
    }
    let cache_time = cache.metadata()?.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    for source in sources {
        let source_time = source.metadata()?.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if cache_time < source_time {
            return Ok(true);
        }
    }
    Ok(false)
}

fn locate_hearthdb_dll(override_path: Option<&Path>) -> Result<PathBuf, CardCatalogError> {
    if let Some(path) = override_path {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
    }
    if let Ok(path) = env::var("HEARTHCOACH_HEARTHDB_DLL") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }

    let mut roots = Vec::new();
    if let Ok(current) = env::current_dir() {
        roots.push(current);
    }
    for key in ["LOCALAPPDATA", "APPDATA", "PROGRAMFILES", "PROGRAMFILES(X86)"] {
        if let Ok(value) = env::var(key) {
            let base = PathBuf::from(value);
            roots.push(base.join("HearthstoneDeckTracker"));
            roots.push(base.join("Hearthstone Deck Tracker"));
            roots.push(base.join("HDT"));
        }
    }

    let mut candidates = Vec::new();
    for root in roots {
        find_named_file(&root, "HearthDb.dll", 7, &mut candidates);
    }

    candidates
        .into_iter()
        .max_by_key(|path| path.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH))
        .ok_or(CardCatalogError::HearthDbNotFound)
}

fn locate_hdt_carddefs_base() -> Option<PathBuf> {
    if let Ok(path) = env::var("HEARTHCOACH_CARDDEFS_BASE") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }

    // HDT Config.AppDataPath = %APPDATA%\HearthstoneDeckTracker.
    let appdata = env::var("APPDATA").ok().map(PathBuf::from)?;
    let root = appdata.join("HearthstoneDeckTracker").join("CardDefs");
    let direct = root.join("CardDefs.base.xml");
    if direct.is_file() {
        return Some(direct);
    }

    let mut candidates = Vec::new();
    find_named_file(&root, "CardDefs.base.xml", 4, &mut candidates);
    candidates.into_iter().max_by_key(|path| {
        path.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH)
    })
}

fn find_named_file(root: &Path, name: &str, depth: usize, out: &mut Vec<PathBuf>) {
    if depth == 0 || !root.exists() {
        return;
    }
    if root.is_file() {
        if root.file_name().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case(name)) == Some(true) {
            out.push(root.to_path_buf());
        }
        return;
    }

    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if path.file_name().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case(name)) == Some(true) {
                out.push(path);
            }
        } else if path.is_dir() {
            find_named_file(&path, name, depth - 1, out);
        }
    }
}

fn export_hdt_catalog(
    dll: &Path,
    carddefs_base: Option<&Path>,
    output: &Path,
) -> Result<(), CardCatalogError> {
    if !cfg!(windows) {
        return Err(CardCatalogError::UnsupportedPlatform);
    }

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }

    let script_path = env::temp_dir().join(format!(
        "hearthcoach_export_hearthdb_{}.ps1",
        std::process::id()
    ));
    fs::write(&script_path, EXPORT_SCRIPT.as_bytes())?;

    let mut last_error = None;
    for shell in ["powershell.exe", "powershell"] {
        let mut command = Command::new(shell);
        command
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&script_path)
            .arg("-HearthDbDll")
            .arg(dll);
        if let Some(carddefs_base) = carddefs_base {
            command.arg("-CardDefsBase").arg(carddefs_base);
        }
        command.arg("-Output").arg(output);

        match command.output() {
            Ok(result) if result.status.success() => {
                let _ = fs::remove_file(&script_path);
                return Ok(());
            }
            Ok(result) => {
                last_error = Some(format!(
                    "{} exited with {}\nstdout:\n{}\nstderr:\n{}",
                    shell,
                    result.status,
                    String::from_utf8_lossy(&result.stdout),
                    String::from_utf8_lossy(&result.stderr)
                ));
            }
            Err(error) => last_error = Some(format!("could not launch {shell}: {error}")),
        }
    }

    let _ = fs::remove_file(&script_path);
    Err(CardCatalogError::PowerShellFailed(
        last_error.unwrap_or_else(|| "unknown PowerShell failure".to_owned()),
    ))
}
