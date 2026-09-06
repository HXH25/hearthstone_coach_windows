use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};

use crate::harness::latest_power_log;

use super::config::DemoConfig;

const POWER_SECTION: &str = "[Power]\r\nLogLevel=1\r\nFilePrinting=True\r\nConsolePrinting=False\r\nScreenPrinting=False\r\nVerbose=True\r\n";
const POWER_RECENT_WINDOW: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnvironmentStatus {
    pub hearthstone_dir: Option<PathBuf>,
    pub hearthstone_exe_found: bool,
    pub detected_by: Option<String>,
    pub log_config_path: Option<PathBuf>,
    pub log_config_exists: bool,
    pub power_logging_ready: bool,
    pub logs_dir: Option<PathBuf>,
    pub latest_power_log: Option<PathBuf>,
    pub latest_power_log_age_ms: Option<u64>,
    pub power_log_recent: bool,
    pub restart_required: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnvironmentRepairResult {
    pub status: EnvironmentStatus,
    pub config_changed: bool,
    pub log_config_changed: bool,
    pub restart_required: bool,
}

pub fn prepare_environment(config: &mut DemoConfig) -> io::Result<EnvironmentRepairResult> {
    let mut result = EnvironmentRepairResult::default();

    let configured = config.hearthstone_dir.clone();
    let detection = detect_hearthstone_dir(&configured);
    if let Some((dir, source)) = detection {
        if config.hearthstone_dir != dir {
            config.hearthstone_dir = dir.clone();
            result.config_changed = true;
        }
        result.status.hearthstone_dir = Some(dir);
        result.status.hearthstone_exe_found = true;
        result.status.detected_by = Some(source);
    } else {
        if !configured.as_os_str().is_empty() {
            result.status.notes.push(format!(
                "配置中的炉石目录不可用：{}",
                configured.display()
            ));
        }
        result.status.notes.push(
            "尚未找到 Hearthstone.exe；启动炉石后点击“自动检测并修复”即可再次检测。"
                .to_owned(),
        );
    }

    if let Some(log_path) = default_log_config_path() {
        result.status.log_config_path = Some(log_path.clone());
        let before = fs::read(&log_path)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        result.status.log_config_exists = log_path.exists();
        result.status.power_logging_ready = power_log_config_ready(&before);
        if !result.status.power_logging_ready {
            let repaired = ensure_power_log_config_text(&before);
            if repaired != strip_utf8_bom(&before) {
                if let Some(parent) = log_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                if let Ok(metadata) = fs::metadata(&log_path) {
                    let mut permissions = metadata.permissions();
                    if permissions.readonly() {
                        permissions.set_readonly(false);
                        let _ = fs::set_permissions(&log_path, permissions);
                    }
                }
                fs::write(&log_path, repaired)?;
                result.log_config_changed = true;
                result.status.log_config_exists = true;
                result.status.power_logging_ready = true;
                result.restart_required = hearthstone_is_running();
                result.status.restart_required = result.restart_required;
                if result.restart_required {
                    result.status.notes.push(
                        "Power 日志配置刚刚被修复；炉石当前正在运行，请重启炉石后再开始对局。"
                            .to_owned(),
                    );
                }
            }
        }
    } else {
        result
            .status
            .notes
            .push("无法解析 LOCALAPPDATA，因此不能自动管理 log.config。".to_owned());
    }

    enrich_power_log_status(config, &mut result.status)?;
    Ok(result)
}

pub fn inspect_environment(config: &DemoConfig) -> EnvironmentStatus {
    let mut status = EnvironmentStatus::default();
    if let Some(dir) = valid_hearthstone_dir(&config.hearthstone_dir) {
        status.hearthstone_dir = Some(dir);
        status.hearthstone_exe_found = true;
        status.detected_by = Some("saved config".to_owned());
    } else if let Some((dir, source)) = detect_hearthstone_dir(&config.hearthstone_dir) {
        status.hearthstone_dir = Some(dir);
        status.hearthstone_exe_found = true;
        status.detected_by = Some(source);
    } else if !config.hearthstone_dir.as_os_str().is_empty() {
        status.hearthstone_dir = Some(config.hearthstone_dir.clone());
    }

    if let Some(path) = default_log_config_path() {
        status.log_config_path = Some(path.clone());
        status.log_config_exists = path.exists();
        let text = fs::read(&path)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        status.power_logging_ready = power_log_config_ready(&text);
    }

    let _ = enrich_power_log_status(config, &mut status);
    status
}

fn enrich_power_log_status(config: &DemoConfig, status: &mut EnvironmentStatus) -> io::Result<()> {
    let dir = status
        .hearthstone_dir
        .clone()
        .or_else(|| valid_hearthstone_dir(&config.hearthstone_dir));
    let Some(dir) = dir else {
        return Ok(());
    };
    let logs = dir.join("Logs");
    status.logs_dir = Some(logs.clone());
    if let Some(power) = latest_power_log(&logs)? {
        let age_ms = power
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .map(|age| age.as_millis().min(u128::from(u64::MAX)) as u64);
        status.latest_power_log_age_ms = age_ms;
        status.power_log_recent = age_ms
            .map(|age| age <= POWER_RECENT_WINDOW.as_millis() as u64)
            .unwrap_or(false);
        status.latest_power_log = Some(power);
    }
    Ok(())
}

pub fn detect_hearthstone_dir(configured: &Path) -> Option<(PathBuf, String)> {
    if let Some(dir) = env::var_os("HEARTHCOACH_HEARTHSTONE_DIR")
        .map(PathBuf::from)
        .and_then(|path| valid_hearthstone_dir(&path))
    {
        return Some((dir, "HEARTHCOACH_HEARTHSTONE_DIR".to_owned()));
    }

    #[cfg(windows)]
    if let Some(dir) = running_hearthstone_dir() {
        return Some((dir, "running Hearthstone process (Win32 Unicode)".to_owned()));
    }

    if let Some(dir) = valid_hearthstone_dir(configured) {
        return Some((dir, "saved config".to_owned()));
    }

    for candidate in common_hearthstone_candidates() {
        if let Some(dir) = valid_hearthstone_dir(&candidate) {
            return Some((dir, "standard install path".to_owned()));
        }
    }
    None
}

fn valid_hearthstone_dir(path: &Path) -> Option<PathBuf> {
    if path.as_os_str().is_empty() {
        return None;
    }
    let path = if path.is_file() {
        path.parent()?.to_path_buf()
    } else {
        path.to_path_buf()
    };
    path.join("Hearthstone.exe").is_file().then_some(path)
}

fn common_hearthstone_candidates() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(windows)]
    {
        for drive in b'C'..=b'Z' {
            let root = format!("{}:\\", drive as char);
            let root = PathBuf::from(root);
            if root.exists() {
                roots.push(root);
            }
        }
    }
    #[cfg(not(windows))]
    {
        roots.push(PathBuf::from("/"));
    }

    let mut out = Vec::new();
    for root in roots {
        out.push(root.join("Hearthstone"));
        out.push(root.join("Games").join("Hearthstone"));
        out.push(root.join("Blizzard").join("Hearthstone"));
        out.push(root.join("Battle.net").join("Hearthstone"));
        out.push(root.join("Program Files").join("Hearthstone"));
        out.push(root.join("Program Files (x86)").join("Hearthstone"));
        out.push(root.join("Program Files").join("Blizzard Entertainment").join("Hearthstone"));
        out.push(
            root.join("Program Files (x86)")
                .join("Blizzard Entertainment")
                .join("Hearthstone"),
        );
    }
    out
}

#[cfg(windows)]
fn running_hearthstone_dir() -> Option<PathBuf> {
    let pid = running_hearthstone_pid()?;
    query_process_image_path(pid).and_then(|path| valid_hearthstone_dir(&path))
}

#[cfg(windows)]
fn running_hearthstone_pid() -> Option<u32> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }

        let mut entry: PROCESSENTRY32W = zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snapshot, &mut entry);
        while ok != 0 {
            let len = entry
                .szExeFile
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            if name.eq_ignore_ascii_case("Hearthstone.exe") {
                let pid = entry.th32ProcessID;
                CloseHandle(snapshot);
                return Some(pid);
            }
            ok = Process32NextW(snapshot, &mut entry);
        }

        CloseHandle(snapshot);
    }
    None
}

#[cfg(windows)]
fn query_process_image_path(pid: u32) -> Option<PathBuf> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };

    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }

        // QueryFullProcessImageNameW works in UTF-16. Construct PathBuf directly
        // from the wide buffer so paths such as D:\应用\Hearthstone never cross
        // an OEM/ANSI/UTF-8 text boundary.
        let mut buffer = vec![0u16; 32_768];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 || len == 0 {
            return None;
        }
        buffer.truncate(len as usize);
        Some(pathbuf_from_wide(&buffer))
    }
}

#[cfg(windows)]
fn pathbuf_from_wide(buffer: &[u16]) -> PathBuf {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    PathBuf::from(OsString::from_wide(buffer))
}

#[cfg(not(windows))]
fn hearthstone_is_running() -> bool {
    false
}

#[cfg(windows)]
fn hearthstone_is_running() -> bool {
    running_hearthstone_pid().is_some()
}

pub fn default_log_config_path() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|root| root.join("Blizzard").join("Hearthstone").join("log.config"))
}

pub fn power_log_config_ready(text: &str) -> bool {
    let normalized = normalize_newlines(text);
    let mut in_power = false;
    let mut log_level = false;
    let mut file_printing = false;
    let mut verbose = false;
    for raw in normalized.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_power = line.eq_ignore_ascii_case("[Power]");
            continue;
        }
        if !in_power {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key.eq_ignore_ascii_case("LogLevel") {
            log_level = value.parse::<u32>().map(|v| v >= 1).unwrap_or(false);
        } else if key.eq_ignore_ascii_case("FilePrinting") {
            file_printing = parse_bool(value);
        } else if key.eq_ignore_ascii_case("Verbose") {
            verbose = parse_bool(value);
        }
    }
    log_level && file_printing && verbose
}

pub fn ensure_power_log_config_text(text: &str) -> String {
    let normalized = normalize_newlines(text);
    let lines = normalized.lines().collect::<Vec<_>>();
    let mut out = Vec::<String>::new();
    let mut i = 0usize;
    let mut replaced = false;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().eq_ignore_ascii_case("[Power]") {
            if !out.is_empty() && !out.last().map(|x| x.is_empty()).unwrap_or(false) {
                out.push(String::new());
            }
            out.extend(POWER_SECTION.lines().map(ToOwned::to_owned));
            replaced = true;
            i += 1;
            while i < lines.len() {
                let next = lines[i].trim();
                if next.starts_with('[') && next.ends_with(']') {
                    break;
                }
                i += 1;
            }
            continue;
        }
        out.push(line.to_owned());
        i += 1;
    }
    if !replaced {
        if !out.is_empty() && !out.last().map(|x| x.is_empty()).unwrap_or(false) {
            out.push(String::new());
        }
        out.extend(POWER_SECTION.lines().map(ToOwned::to_owned));
    }
    while out.last().map(|line| line.is_empty()).unwrap_or(false) {
        out.pop();
    }
    let mut result = out.join("\r\n");
    result.push_str("\r\n");
    result
}

fn normalize_newlines(text: &str) -> String {
    strip_utf8_bom(text)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

fn strip_utf8_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

fn parse_bool(value: &str) -> bool {
    value.eq_ignore_ascii_case("true") || value == "1"
}

#[cfg(test)]
mod tests {
    use super::{ensure_power_log_config_text, power_log_config_ready};

    #[test]
    fn repairs_missing_power_section_without_dropping_other_sections() {
        let input = "[LoadingScreen]\nLogLevel=1\nFilePrinting=True\n";
        let fixed = ensure_power_log_config_text(input);
        assert!(fixed.contains("[LoadingScreen]"));
        assert!(fixed.contains("[Power]"));
        assert!(power_log_config_ready(&fixed));
    }

    #[test]
    fn repairs_bad_power_section_and_preserves_following_section() {
        let input = "[Power]\nLogLevel=0\nFilePrinting=False\nVerbose=False\n[Zone]\nLogLevel=1\n";
        let fixed = ensure_power_log_config_text(input);
        assert!(power_log_config_ready(&fixed));
        assert!(fixed.contains("[Zone]"));
        assert!(!fixed.contains("FilePrinting=False"));
    }

    #[cfg(windows)]
    #[test]
    fn unicode_process_path_roundtrips_without_text_encoding() {
        use super::pathbuf_from_wide;
        let expected = r"D:\应用\Hearthstone\Hearthstone.exe";
        let wide = expected.encode_utf16().collect::<Vec<_>>();
        assert_eq!(pathbuf_from_wide(&wide).to_string_lossy(), expected);
    }
}
