use std::{
    env,
    error::Error,
    fmt,
    fs,
    io,
    path::{Path, PathBuf},
    process::Command,
};

use crate::harness::card_catalog::normalize_tribe;

const READ_SCRIPT: &str = include_str!("../../tools/read_available_tribes.ps1");

#[derive(Debug)]
pub enum AvailableTribesError {
    Io(io::Error),
    HearthMirrorNotFound,
    PowerShellFailed(String),
    Json(serde_json::Error),
    UnsupportedPlatform,
}

impl fmt::Display for AvailableTribesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::HearthMirrorNotFound => write!(
                f,
                "could not find HDT's HearthMirror.dll; pass --hearthmirror-dll or set HEARTHCOACH_HEARTHMIRROR_DLL"
            ),
            Self::PowerShellFailed(message) => write!(f, "HearthMirror query failed: {message}"),
            Self::Json(error) => write!(f, "HearthMirror JSON error: {error}"),
            Self::UnsupportedPlatform => write!(f, "automatic HearthMirror query is only supported on Windows"),
        }
    }
}

impl Error for AvailableTribesError {}

impl From<io::Error> for AvailableTribesError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for AvailableTribesError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

/// Optional enrichment provider. Power.log remains authoritative for match
/// history; HearthMirror is queried only for the client-visible tribe pool that
/// HDT itself obtains from `Reflection.Client.GetAvailableBattlegroundsRaces()`.
#[derive(Debug, Clone)]
pub struct HearthMirrorAvailableTribesProvider {
    dll: PathBuf,
}

impl HearthMirrorAvailableTribesProvider {
    pub fn auto(override_path: Option<&Path>) -> Result<Self, AvailableTribesError> {
        Ok(Self {
            dll: locate_hearthmirror_dll(override_path)?,
        })
    }

    pub fn dll_path(&self) -> &Path {
        &self.dll
    }

    pub fn read(&self) -> Result<Option<Vec<String>>, AvailableTribesError> {
        if !cfg!(windows) {
            return Err(AvailableTribesError::UnsupportedPlatform);
        }

        let script_path = write_temp_script()?;
        let output = Command::new("powershell.exe")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&script_path)
            .arg("-HearthMirrorDll")
            .arg(&self.dll)
            .output()?;
        let _ = fs::remove_file(&script_path);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            return Err(AvailableTribesError::PowerShellFailed(if stderr.is_empty() {
                stdout
            } else {
                stderr
            }));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let raw = stdout.trim().trim_start_matches('\u{feff}');
        if raw.is_empty() || raw == "null" {
            return Ok(None);
        }

        // PowerShell emits a bare JSON string for a one-element array. Accept
        // both shapes even though Battlegrounds normally has multiple races.
        let raw_values: Vec<String> = match serde_json::from_str::<Vec<String>>(raw) {
            Ok(values) => values,
            Err(_) => vec![serde_json::from_str::<String>(raw)?],
        };
        let mut tribes = raw_values
            .iter()
            .filter_map(|value| normalize_tribe(value))
            .collect::<Vec<_>>();
        tribes.sort();
        tribes.dedup();
        if tribes.is_empty() {
            Ok(None)
        } else {
            Ok(Some(tribes))
        }
    }
}

pub fn locate_hearthmirror_dll(override_path: Option<&Path>) -> Result<PathBuf, AvailableTribesError> {
    if let Some(path) = override_path {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
    }
    if let Ok(path) = env::var("HEARTHCOACH_HEARTHMIRROR_DLL") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
    }

    let mut roots = Vec::new();
    if let Ok(local) = env::var("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("HearthstoneDeckTracker"));
    }
    if let Ok(roaming) = env::var("APPDATA") {
        roots.push(PathBuf::from(roaming).join("HearthstoneDeckTracker"));
    }

    let mut candidates = Vec::new();
    for root in roots {
        collect_named_files(&root, "HearthMirror.dll", 3, &mut candidates)?;
    }
    candidates.sort_by_key(|path| {
        path.metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    candidates.pop().ok_or(AvailableTribesError::HearthMirrorNotFound)
}

fn collect_named_files(
    root: &Path,
    file_name: &str,
    depth: usize,
    out: &mut Vec<PathBuf>,
) -> io::Result<()> {
    if depth == 0 || !root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_named_files(&path, file_name, depth - 1, out)?;
        } else if path.file_name().and_then(|name| name.to_str()) == Some(file_name) {
            out.push(path);
        }
    }
    Ok(())
}

fn write_temp_script() -> io::Result<PathBuf> {
    let mut path = env::temp_dir();
    let unique = format!(
        "hearthcoach-read-tribes-{}-{}.ps1",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    path.push(unique);
    fs::write(&path, READ_SCRIPT)?;
    Ok(path)
}
