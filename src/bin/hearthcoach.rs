use std::{thread, time::Duration};

use hearthcoach_harness::{
    demo::{
        config::DemoConfig,
        environment::{detect_hearthstone_dir, prepare_environment},
        control_center::run_control_center,
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
        eprintln!("HearthCoach error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = DemoConfig::config_path();
    let mut config = DemoConfig::load_or_create(&config_path)?;
    match prepare_environment(&mut config) {
        Ok(result) => {
            if result.config_changed {
                config.save(&config_path)?;
            }
            if result.log_config_changed {
                eprintln!("[environment] Power logging config repaired: {}",
                    result.status.log_config_path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "unknown".to_owned()));
            }
            if result.restart_required {
                eprintln!("[environment] Hearthstone is running; restart Hearthstone once so the repaired log.config takes effect.");
            }
            for note in result.status.notes {
                eprintln!("[environment] {note}");
            }
        }
        Err(error) => eprintln!("[environment] automatic environment preparation failed: {error}"),
    }
    let catalog = CardCatalog::load_or_export_hdt(None, false)?;
    let knowledge = HdtKnowledgeBase::from_catalog(catalog.clone());
    let state = DemoState::new(config.clone(), config_path.clone(), catalog.clone());

    println!("HearthCoach V0.5.0.7 · Latest Power.log + Manual Refresh + Course Compliance");
    println!("Config      : {}", config_path.display());
    println!("Hearthstone : {}", config.hearthstone_dir.display());
    println!("Model       : {}", config.deepseek.model);
    println!("Endpoint    : {}", config.deepseek.base_url);
    println!("Card DB     : {} cards", catalog.len());
    println!("HDT BG pool : {} current normal minions", knowledge.stats().bacon_pool_minions);
    println!("History     : {}", config.compliance.history_dir.display());

    let server_state = state.clone();
    thread::spawn(move || {
        if let Err(error) = run_server(server_state) {
            eprintln!("[server] {error}");
        }
    });

    // Game UI remains the lightweight overlay developed in V0.4.x.
    spawn_overlay(state.clone());

    let monitor_state = state.clone();
    thread::spawn(move || monitor_forever(monitor_state, catalog));

    // The desktop control center is the primary out-of-match UI. It does not
    // overlay Hearthstone; users may minimize it like HDT while playing.
    run_control_center(state).map_err(|error| error.into())
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
            if let Err(error) = state.repair_environment() {
                eprintln!("[environment] repair retry failed: {error}");
            }
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
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                // No match is active. Re-enter the outer loop so a moved/new
                // Hearthstone install or repaired logging config is picked up.
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                thread::sleep(Duration::from_secs(2));
            }
            Err(error) => {
                eprintln!("[monitor] {error}; re-detecting environment in 2 seconds");
                thread::sleep(Duration::from_secs(2));
            }
        }
    }
}
