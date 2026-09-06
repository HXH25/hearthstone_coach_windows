use std::{process::Command, thread, time::Duration};

use hearthcoach_harness::{
    demo::{
        config::DemoConfig,
        environment::{detect_hearthstone_dir, prepare_environment},
        hdt_knowledge::HdtKnowledgeBase,
        overlay::spawn_overlay,
        server::{run_server, DemoState},
    },
    harness::{
        latest_power_log, watch_one_match_with_catalog_and_tribes_state, CardCatalog,
        HearthMirrorAvailableTribesProvider, WatchConfig,
    },
};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = DemoConfig::config_path();
    let mut config = DemoConfig::load_or_create(&config_path)?;
    if let Ok(result) = prepare_environment(&mut config) {
        if result.config_changed {
            config.save(&config_path)?;
        }
        if result.restart_required {
            eprintln!("[environment] restart Hearthstone once so the repaired Power logging config takes effect");
        }
    }
    let catalog = CardCatalog::load_or_export_hdt(None, false)?;
    let knowledge = HdtKnowledgeBase::from_catalog(catalog.clone());
    let state = DemoState::new(config.clone(), config_path.clone(), catalog.clone());

    println!("HearthCoach Demo Compatibility Entry · V0.5.0.7 Log Refresh");
    println!("Config      : {}", config_path.display());
    println!("Hearthstone : {}", config.hearthstone_dir.display());
    println!("Model       : {}", config.deepseek.model);
    println!("API key     : {}", if config.deepseek.api_key.trim().is_empty() { "NOT CONFIGURED" } else { "configured" });
    println!("Card DB     : {} cards", catalog.len());
    println!("HDT BG pool : {} current normal minions", knowledge.stats().bacon_pool_minions);
    println!("Knowledge   : {}", knowledge.stats().source);
    if knowledge.stats().bacon_pool_minions == 0 {
        eprintln!("WARNING: HDT BaconPoolMinions is empty. Run `cargo run -- carddb rebuild` and verify HDT CardDefs.base.xml.");
    }
    println!("Control UI  : http://127.0.0.1:{}", config.port);
    println!("\nKeep this window running, then start a Battlegrounds match.\n");

    let server_state = state.clone();
    thread::spawn(move || {
        if let Err(error) = run_server(server_state) {
            eprintln!("[demo server] {error}");
        }
    });
    spawn_overlay(state.clone());
    if config.open_browser_on_start {
        open_browser(config.port);
    }

    let monitor_state = state.clone();
    thread::spawn(move || monitor_forever(monitor_state, catalog));

    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

fn monitor_forever(state: DemoState, catalog: CardCatalog) {
    loop {
        let config = state.config();
        let configured_dir_valid = config.hearthstone_dir.join("Hearthstone.exe").is_file();
        let status = state.environment_status();
        let detected_dir_differs = detect_hearthstone_dir(&config.hearthstone_dir)
            .map(|(dir, _)| dir != config.hearthstone_dir)
            .unwrap_or(false);
        if !configured_dir_valid || detected_dir_differs || !status.power_logging_ready {
            let _ = state.repair_environment();
            thread::sleep(Duration::from_secs(2));
            continue;
        }
        let config = state.config();
        let watch_config = WatchConfig::new(config.hearthstone_dir.clone())
            .with_force_refresh_generation(state.log_refresh_generation_handle());
        let latest_at_start = latest_power_log(&watch_config.logs_dir()).ok().flatten();
        state.set_watched_power_log(latest_at_start);
        let provider = HearthMirrorAvailableTribesProvider::auto(None).ok();
        let event_state = state.clone();
        let provider_state = state.clone();
        let result = watch_one_match_with_catalog_and_tribes_state(
            &watch_config,
            catalog.clone(),
            move |event, runtime| event_state.observe_runtime(event, runtime),
            move || {
                let provider = provider.as_ref()?;
                let tribes = provider.read().ok().flatten()?;
                provider_state.set_available_tribes(tribes.clone());
                Some((tribes, format!("HearthMirror:{}", provider.dll_path().display())))
            },
        );
        match result {
            Ok((_archive, path)) => eprintln!("[monitor] match archived from {}", path.display()),
            Err(error) if matches!(
                error.kind(),
                std::io::ErrorKind::Interrupted | std::io::ErrorKind::NotFound
            ) => {
                thread::sleep(Duration::from_secs(2));
            }
            Err(error) => {
                eprintln!("[monitor] {error}; retrying in 2 seconds");
                thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

fn open_browser(port: u16) {
    let url = format!("http://127.0.0.1:{port}");
    #[cfg(windows)]
    {
        let _ = Command::new("cmd").args(["/C", "start", "", &url]).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(&url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = Command::new("xdg-open").arg(&url).spawn();
    }
}
