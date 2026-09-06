use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use crate::harness::{CardCatalog, GameArchive, HarnessEvent, HarnessRuntime};

const POWER_MARKER: &str = "GameState.DebugPrintPower() - ";

#[derive(Debug, Clone)]
pub struct WatchConfig {
    pub hearthstone_dir: PathBuf,
    pub poll_interval: Duration,
    /// After STATE=COMPLETE, keep reading briefly because Hearthstone may emit
    /// final leaderboard tags a few lines later.
    pub completion_grace: Duration,
    /// How often an optional external AvailableTribes provider is retried until
    /// it succeeds for the active match.
    pub tribe_poll_interval: Duration,
    /// While no match is active, periodically return control to the caller so
    /// a changed Hearthstone installation/config can be re-detected.
    pub source_refresh_interval: Duration,
    /// Optional cross-thread generation counter. When it changes while this
    /// watcher is running, the watcher safely yields so the caller can rescan
    /// and reopen the newest Power.log. Used by the Control Center “刷新日志”.
    pub force_refresh_generation: Option<Arc<AtomicU64>>,
}

impl WatchConfig {
    pub fn new(hearthstone_dir: impl Into<PathBuf>) -> Self {
        Self {
            hearthstone_dir: hearthstone_dir.into(),
            poll_interval: Duration::from_millis(100),
            completion_grace: Duration::from_secs(2),
            tribe_poll_interval: Duration::from_secs(1),
            source_refresh_interval: Duration::from_secs(3),
            force_refresh_generation: None,
        }
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.hearthstone_dir.join("Logs")
    }

    pub fn with_force_refresh_generation(mut self, generation: Arc<AtomicU64>) -> Self {
        self.force_refresh_generation = Some(generation);
        self
    }
}

/// Library-compatible watcher using only an already cached card catalog (if
/// present). Available tribes remain None unless the caller injects them.
pub fn watch_one_match<F>(config: &WatchConfig, on_event: F) -> io::Result<(GameArchive, PathBuf)>
where
    F: FnMut(&HarnessEvent),
{
    let catalog = CardCatalog::load_cached_default().unwrap_or_default();
    watch_one_match_with_catalog(config, catalog, on_event)
}

pub fn watch_one_match_with_catalog<F>(
    config: &WatchConfig,
    catalog: CardCatalog,
    on_event: F,
) -> io::Result<(GameArchive, PathBuf)>
where
    F: FnMut(&HarnessEvent),
{
    watch_one_match_with_catalog_and_tribes(config, catalog, on_event, || None)
}

/// Wait for one match and archive it. The `tribe_provider` is deliberately
/// optional and separate from Power.log parsing. It may return a client-visible
/// tribe list plus an audit/source string; the watcher retries it periodically
/// until one successful result is obtained.
pub fn watch_one_match_with_catalog_and_tribes<F, P>(
    config: &WatchConfig,
    catalog: CardCatalog,
    mut on_event: F,
    tribe_provider: P,
) -> io::Result<(GameArchive, PathBuf)>
where
    F: FnMut(&HarnessEvent),
    P: FnMut() -> Option<(Vec<String>, String)>,
{
    watch_one_match_with_catalog_and_tribes_state(
        config,
        catalog,
        move |event, _runtime| on_event(event),
        tribe_provider,
    )
}

/// State-aware variant used by the Agent layer. The callback receives the
/// immutable Runtime immediately after each semantic event is committed, so
/// downstream code can consume projected snapshots without touching EntityStore.
pub fn watch_one_match_with_catalog_and_tribes_state<F, P>(
    config: &WatchConfig,
    catalog: CardCatalog,
    mut on_event: F,
    mut tribe_provider: P,
) -> io::Result<(GameArchive, PathBuf)>
where
    F: FnMut(&HarnessEvent, &HarnessRuntime),
    P: FnMut() -> Option<(Vec<String>, String)>,
{
    let logs_dir = config.logs_dir();
    if !logs_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Hearthstone Logs directory not found: {}", logs_dir.display()),
        ));
    }
    let watch_started = Instant::now();
    let refresh_generation_at_start = config
        .force_refresh_generation
        .as_ref()
        .map(|generation| generation.load(Ordering::SeqCst));
    let initial_path = latest_power_log(&logs_dir)?;
    let followed_path = initial_path.clone();

    let mut reader = match initial_path.as_deref() {
        Some(path) => {
            if let Some(offset) = active_match_start_offset(path)? {
                Some(open_reader_at(path, offset)?)
            } else {
                Some(open_reader_at_end(path)?)
            }
        }
        None => None,
    };

    let mut runtime = HarnessRuntime::with_catalog(
        followed_path.as_ref().map(|path| path.display().to_string()),
        catalog.clone(),
    );
    let mut line = String::new();
    let mut complete_idle_since: Option<Instant> = None;
    let mut last_tribe_poll: Option<Instant> = None;

    loop {
        let latest = latest_power_log(&logs_dir)?;

        // Always follow the newest physical Power.log. A Hearthstone client
        // restart creates a new log file; the previous runtime may still think
        // its match is active if the client exited before STATE=COMPLETE. The
        // old V0.5.0.6 guard (`!runtime.is_match_active()`) could therefore pin
        // the monitor to a dead file forever. Yield immediately instead.
        if latest != followed_path {
            interrupt_runtime_for_source_refresh(
                &mut runtime,
                &mut on_event,
                "检测到更新的 Power.log，切换到最新日志",
            );
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "newer Power.log detected; refresh log source",
            ));
        }

        let manual_refresh_requested = match (
            config.force_refresh_generation.as_ref(),
            refresh_generation_at_start,
        ) {
            (Some(generation), Some(start)) => generation.load(Ordering::SeqCst) != start,
            _ => false,
        };
        if manual_refresh_requested {
            interrupt_runtime_for_source_refresh(
                &mut runtime,
                &mut on_event,
                "用户请求刷新 Power.log",
            );
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "manual Power.log refresh requested",
            ));
        }

        let Some(active_reader) = reader.as_mut() else {
            // No Power.log exists yet. Do not stay pinned to this Logs directory
            // forever: when the user starts Hearthstone from another install after
            // HearthCoach, return to the outer environment detector periodically.
            if !runtime.is_match_active()
                && watch_started.elapsed() >= config.source_refresh_interval
            {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "refresh Hearthstone log source",
                ));
            }
            thread::sleep(config.poll_interval);
            continue;
        };

        let mut read_any = false;
        loop {
            line.clear();
            let bytes = active_reader.read_line(&mut line)?;
            if bytes == 0 {
                break;
            }
            read_any = true;
            let line = line.trim_end_matches(|c| c == '\r' || c == '\n');
            for event in runtime.feed_line(line) {
                on_event(&event, &runtime);
            }
        }

        // The current EOF is our debounce clock. Runtime emits ShopUpdated only
        // after no shop entity changed for >=150 ms, avoiding the partial shops
        // observed immediately after Refresh blocks.
        for event in runtime.on_idle() {
            on_event(&event, &runtime);
        }

        if runtime.is_match_active() && runtime.available_tribes().is_none() {
            let should_poll = last_tribe_poll
                .map(|instant| instant.elapsed() >= config.tribe_poll_interval)
                .unwrap_or(true);
            if should_poll {
                last_tribe_poll = Some(Instant::now());
                if let Some((tribes, source)) = tribe_provider() {
                    if !tribes.is_empty() {
                        runtime.set_available_tribes_with_source(tribes, source);
                    }
                }
            }
        }

        if runtime.is_complete_seen() {
            if read_any {
                complete_idle_since = Some(Instant::now());
            } else {
                let since = complete_idle_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= config.completion_grace {
                    for event in runtime.finish(None) {
                        on_event(&event, &runtime);
                    }
                    let path = followed_path.clone().ok_or_else(|| {
                        io::Error::new(io::ErrorKind::NotFound, "Power.log disappeared")
                    })?;
                    return Ok((runtime.archive().clone(), path));
                }
            }
        }

        if !runtime.is_match_active()
            && watch_started.elapsed() >= config.source_refresh_interval
        {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "refresh Hearthstone log source",
            ));
        }

        thread::sleep(config.poll_interval);
    }
}

fn interrupt_runtime_for_source_refresh<F>(
    runtime: &mut HarnessRuntime,
    on_event: &mut F,
    reason: &str,
) where
    F: FnMut(&HarnessEvent, &HarnessRuntime),
{
    if !runtime.is_match_active() {
        return;
    }
    // Flush the partial round/shop so the archived interrupted session remains
    // useful for R5 audit/history, then emit an explicit lifecycle boundary.
    for event in runtime.finish(None) {
        on_event(&event, runtime);
    }
    on_event(
        &HarnessEvent::MatchInterrupted {
            reason: reason.to_owned(),
        },
        runtime,
    );
}

fn open_reader_at(path: &Path, offset: u64) -> io::Result<BufReader<File>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(offset))?;
    Ok(reader)
}

fn open_reader_at_end(path: &Path) -> io::Result<BufReader<File>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::End(0))?;
    Ok(reader)
}

/// Returns the byte offset of the newest CREATE_GAME whose STATE=COMPLETE has
/// not yet been observed. This is the recovery anchor used when the Rust
/// process starts after a match has already begun.
pub fn active_match_start_offset(path: &Path) -> io::Result<Option<u64>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut last_create: Option<u64> = None;
    let mut last_complete: Option<u64> = None;

    loop {
        let offset = reader.stream_position()?;
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let Some((_, payload)) = line.split_once(POWER_MARKER) else {
            continue;
        };
        let payload = payload.trim();
        if payload == "CREATE_GAME" || payload.starts_with("CREATE_GAME ") {
            last_create = Some(offset);
        }
        if payload.starts_with("TAG_CHANGE Entity=GameEntity")
            && payload.contains(" tag=STATE ")
            && payload.contains(" value=COMPLETE")
        {
            last_complete = Some(offset);
        }
    }

    Ok(match (last_create, last_complete) {
        (Some(create), Some(complete)) if create > complete => Some(create),
        (Some(create), None) => Some(create),
        _ => None,
    })
}

/// Find the newest Power.log recursively. Hearthstone/HDT have changed the
/// exact Logs subdirectory shape over time, so the watcher must not assume a
/// single `Logs/Hearthstone_timestamp/Power.log` depth.
pub fn latest_power_log(logs_dir: &Path) -> io::Result<Option<PathBuf>> {
    if !logs_dir.exists() {
        return Ok(None);
    }

    let mut candidates = Vec::new();
    collect_power_logs(logs_dir, 5, &mut candidates)?;
    Ok(candidates
        .into_iter()
        .max_by(|(time_a, path_a), (time_b, path_b)| {
            time_a.cmp(time_b).then_with(|| path_a.cmp(path_b))
        })
        .map(|(_, path)| path))
}

fn collect_power_logs(
    dir: &Path,
    depth: usize,
    out: &mut Vec<(SystemTime, PathBuf)>,
) -> io::Result<()> {
    if depth == 0 || !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_power_logs(&path, depth - 1, out)?;
        } else if path.file_name().and_then(|name| name.to_str()) == Some("Power.log") {
            let modified = path.metadata()?.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            out.push((modified, path));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{active_match_start_offset, latest_power_log};
    use std::{env, fs, time::{SystemTime, UNIX_EPOCH}};

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!("{prefix}-{suffix}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn finds_latest_unfinished_create_game() {
        let dir = temp_dir("hearthcoach-monitor");
        let path = dir.join("Power.log");
        let text = concat!(
            "D 10:00:00 GameState.DebugPrintPower() - CREATE_GAME\n",
            "D 10:00:01 GameState.DebugPrintPower() - TAG_CHANGE Entity=GameEntity tag=STATE value=COMPLETE\n",
            "D 10:01:00 GameState.DebugPrintPower() - CREATE_GAME\n",
            "D 10:01:01 GameState.DebugPrintPower() - TAG_CHANGE Entity=GameEntity tag=TURN value=1\n"
        );
        fs::write(&path, text).unwrap();
        let offset = active_match_start_offset(&path).unwrap().unwrap();
        let expected = text.find("D 10:01:00").unwrap() as u64;
        assert_eq!(offset, expected);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn power_log_discovery_is_recursive() {
        let root = temp_dir("hearthcoach-logs");
        let nested = root.join("a").join("b").join("c");
        fs::create_dir_all(&nested).unwrap();
        let path = nested.join("Power.log");
        fs::write(&path, "test").unwrap();
        assert_eq!(latest_power_log(&root).unwrap(), Some(path));
        let _ = fs::remove_dir_all(root);
    }
}
