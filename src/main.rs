use std::{
    env,
    fs::File,
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
};

use hearthcoach_harness::demo::environment::detect_hearthstone_dir;
use hearthcoach_harness::harness::{
    watch_one_match_with_catalog_and_tribes, write_archive_files, ArchivePaths, CardCatalog,
    normalize_tribe, GameArchive, HarnessEvent, HarnessRuntime, HearthMirrorAvailableTribesProvider,
    WatchConfig,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_help();
        return Ok(());
    }

    match args[1].as_str() {
        "watch" => {
            let hearthstone_dir = option_path(&args, "--hearthstone-dir")
                .or_else(|| detect_hearthstone_dir(Path::new("")).map(|(path, _)| path))
                .ok_or("could not auto-detect Hearthstone; pass --hearthstone-dir or start Hearthstone once")?;
            let output = option_path(&args, "--output").unwrap_or_else(|| PathBuf::from("records"));
            let catalog = load_hdt_catalog(&args, false)?;
            let manual_tribes = option_value(&args, "--tribes").map(parse_tribes);

            let mirror_override = option_path(&args, "--hearthmirror-dll");
            let mirror_provider = if manual_tribes.is_none() {
                match HearthMirrorAvailableTribesProvider::auto(mirror_override.as_deref()) {
                    Ok(provider) => {
                        println!("Tribe source : HearthMirror ({})", provider.dll_path().display());
                        Some(provider)
                    }
                    Err(error) => {
                        eprintln!("warning: AvailableTribes provider unavailable: {error}");
                        eprintln!("         match capture will continue; use --tribes DRAGON,MECH,... as fallback.");
                        None
                    }
                }
            } else {
                println!("Tribe source : manual --tribes");
                None
            };

            println!("HearthCoach Harness V0.4.2");
            println!("Hearthstone : {}", hearthstone_dir.display());
            println!("Output      : {}", output.display());
            println!("Card DB     : {} cards", catalog.len());
            println!("Card source : {}", catalog.source().unwrap_or("unknown"));
            println!("Waiting for a Battlegrounds match. Start this before entering the match.\n");

            let config = WatchConfig::new(hearthstone_dir);
            let manual_for_provider = manual_tribes.clone();
            let (archive, source) = watch_one_match_with_catalog_and_tribes(
                &config,
                catalog,
                print_runtime_event,
                move || {
                    if let Some(tribes) = manual_for_provider.as_ref() {
                        return Some((tribes.clone(), "manual --tribes".to_owned()));
                    }
                    let provider = mirror_provider.as_ref()?;
                    provider.read().ok().flatten().map(|tribes| {
                        (
                            tribes,
                            format!("HearthMirror:{}", provider.dll_path().display()),
                        )
                    })
                },
            )?;

            println!("\nMatch finished from: {}", source.display());
            let paths = write_archive_files(&archive, &output)?;
            print_output_paths(&archive, &paths);
        }
        "replay" => {
            let path = args.get(2).ok_or("missing raw Power.log path")?;
            let path = PathBuf::from(path);
            let output = option_path(&args, "--output").unwrap_or_else(|| PathBuf::from("records"));
            let catalog = load_hdt_catalog(&args, false)?;
            let manual_tribes = option_value(&args, "--tribes").map(parse_tribes);
            println!("Card DB     : {} cards", catalog.len());
            println!("Card source : {}", catalog.source().unwrap_or("unknown"));
            let archive = replay(&path, catalog, manual_tribes)?;
            let paths = write_archive_files(&archive, &output)?;
            print_output_paths(&archive, &paths);
        }
        "carddb" => {
            let sub = args.get(2).map(String::as_str).unwrap_or("status");
            match sub {
                "status" => {
                    let dll = CardCatalog::locate_hdt_hearthdb_dll(
                        option_path(&args, "--hearthdb-dll").as_deref(),
                    )?;
                    println!("HDT HearthDb.dll : {}", dll.display());
                    println!(
                        "HDT CardDefs      : {}",
                        CardCatalog::locate_hdt_carddefs_base()
                            .as_deref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "(using HearthDb.dll bundled snapshot)".to_owned())
                    );
                    println!("Rust cache        : {}", CardCatalog::default_cache_path().display());
                    let catalog = CardCatalog::load_or_export_hdt(Some(&dll), false)?;
                    println!("Cards             : {}", catalog.len());
                    println!("Bacon pool minions: {}", catalog.cards().filter(|card| card.in_bacon_pool && !card.premium).count());
                    println!("Source            : {}", catalog.source().unwrap_or("unknown"));
                }
                "rebuild" => {
                    let catalog = load_hdt_catalog(&args, true)?;
                    println!("Rebuilt {} cards", catalog.len());
                    println!("Current BaconPoolMinions: {}", catalog.cards().filter(|card| card.in_bacon_pool && !card.premium).count());
                    println!("Source: {}", catalog.source().unwrap_or("unknown"));
                }
                other => return Err(format!("unknown carddb command: {other}").into()),
            }
        }
        "card" => {
            let card_id = args.get(2).ok_or("missing CardId")?;
            let catalog = load_hdt_catalog(&args, false)?;
            let Some(card) = catalog.resolve(card_id) else {
                return Err(format!("CardId not found in HearthDb: {card_id}").into());
            };
            println!("CardId       : {}", card_id);
            println!("Name         : {}", card.preferred_name().unwrap_or("unknown"));
            println!("Type         : {:?}", card.card_type());
            println!("Tribes       : {:?}", card.tribes());
            println!("Tavern tier  : {:?}", card.tavern_tier());
            println!("Printed cost : {:?}", card.cost());
            println!("Attack/Health: {:?}/{:?}", card.attack(), card.health());
            println!("Golden       : {}", card.premium());
            println!("In BG pool   : {}", card.in_bacon_pool());
            println!("Normal CardId: {:?}", card.normal_card_id());
            println!("Mechanics    : {:?}", card.mechanics());
            println!("Text         : {}", card.preferred_text().unwrap_or(""));
        }
        "tribes" => {
            let sub = args.get(2).map(String::as_str).unwrap_or("status");
            if sub != "status" {
                return Err(format!("unknown tribes command: {sub}").into());
            }
            let provider = HearthMirrorAvailableTribesProvider::auto(
                option_path(&args, "--hearthmirror-dll").as_deref(),
            )?;
            println!("HearthMirror.dll : {}", provider.dll_path().display());
            println!("Available tribes : {:?}", provider.read()?);
            println!("Note: run this while a Battlegrounds lobby/match is active.");
        }
        "help" | "--help" | "-h" => print_help(),
        other => return Err(format!("unknown command: {other}").into()),
    }

    Ok(())
}

fn load_hdt_catalog(
    args: &[String],
    force_rebuild: bool,
) -> Result<CardCatalog, Box<dyn std::error::Error>> {
    let override_path = option_path(args, "--hearthdb-dll");
    Ok(CardCatalog::load_or_export_hdt(
        override_path.as_deref(),
        force_rebuild,
    )?)
}

fn replay(
    path: &Path,
    catalog: CardCatalog,
    manual_tribes: Option<Vec<String>>,
) -> io::Result<GameArchive> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut runtime = HarnessRuntime::with_catalog(Some(path.display().to_string()), catalog);
    let mut last_timestamp = None;

    for line in reader.lines() {
        let line = line?;
        if let Some(timestamp) = hearthcoach_harness::harness::extract_timestamp(&line) {
            last_timestamp = Some(timestamp);
        }
        for event in runtime.feed_line(&line) {
            print_runtime_event(&event);
        }
    }

    for event in runtime.flush_pending_shop_revision() {
        print_runtime_event(&event);
    }
    runtime.finish(last_timestamp);
    if let Some(tribes) = manual_tribes {
        runtime.set_available_tribes_with_source(tribes, "manual --tribes");
    }
    Ok(runtime.archive().clone())
}

fn print_runtime_event(event: &HarnessEvent) {
    match event {
        HarnessEvent::MatchStarted => println!("[match] started"),
        HarnessEvent::MatchInterrupted { reason } => println!("[match] interrupted: {reason}"),
        HarnessEvent::PhaseStarted { round_number, phase } => {
            println!("[round {round_number}] phase -> {phase:?}")
        }
        HarnessEvent::ChoiceOpened { id } => println!("[choice {id}] opened"),
        HarnessEvent::ChoiceUpdated { id } => println!("[choice {id}] updated"),
        HarnessEvent::ChoiceResolved { id } => println!("[choice {id}] resolved"),
        HarnessEvent::RecruitAction { round_number, kind } => {
            println!("[round {round_number}] action {kind:?}")
        }
        HarnessEvent::ShopUpdated { round_number, revision, card_ids } => {
            println!("[round {round_number}] stable shop #{revision}: {}", card_ids.join(", "))
        }
        HarnessEvent::RoundArchived { round_number } => println!("[round {round_number}] archived"),
        HarnessEvent::MatchCompleted => println!("[match] STATE=COMPLETE"),
    }
}

fn option_path(args: &[String], flag: &str) -> Option<PathBuf> {
    option_value(args, flag).map(PathBuf::from)
}

fn option_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let index = args.iter().position(|arg| arg == flag)?;
    args.get(index + 1).map(String::as_str)
}

fn parse_tribes(raw: &str) -> Vec<String> {
    let mut tribes = raw
        .split(',')
        .filter_map(normalize_tribe)
        .collect::<Vec<_>>();
    tribes.sort();
    tribes.dedup();
    tribes
}

fn print_output_paths(archive: &GameArchive, paths: &ArchivePaths) {
    println!("\n=== Archive summary ===");
    println!("schema      : {}", archive.schema_version);
    println!("rounds      : {}", archive.rounds.len());
    println!("choices     : {}", archive.choices().count());
    println!("local player: {:?} {:?}", archive.local_player_id, archive.local_player_name);
    println!("final place : {:?}", archive.final_place);
    println!("card catalog: {:?}", archive.card_catalog_source);
    println!("tribes      : {:?}", archive.available_tribes);
    println!("tribe source: {:?}", archive.available_tribes_source);
    println!("block resets : {}", archive.stale_block_stack_recoveries);
    println!("JSON        : {}", paths.json.display());
    println!("Markdown    : {}", paths.markdown.display());
}

fn print_help() {
    println!(
        r#"HearthCoach Harness V0.4.2

PRIMARY LIVE USAGE (recommended):
  cargo run -- watch --hearthstone-dir "D:\Hearthstone" --output ".\records_v042"

The watcher recursively discovers the newest Power.log. You do NOT need to
manually provide a Logs/.../Power.log path for normal use.

OPTIONAL AVAILABLE-TRIBES DIAGNOSTIC:
  cargo run -- tribes status
  cargo run -- tribes status --hearthmirror-dll "C:\path\to\HearthMirror.dll"

Manual fallback if HearthMirror cannot be queried independently:
  cargo run -- watch --hearthstone-dir "D:\Hearthstone" --tribes "DRAGON,MECH,ELEMENTALS,QUILLBOAR,NAGA"

HDT CARD DATABASE:
  cargo run -- carddb status
  cargo run -- carddb rebuild
  cargo run -- card BG20_HERO_102

OFFLINE REPLAY (only when you explicitly have a raw Power.log):
  cargo run -- replay "D:\...\Power.log" --output ".\records_v042"

TESTS:
  cargo test

V0.4.2 CORRECTNESS/FREEZE TARGET:
  * One RoundArchive per Battlegrounds round (Recruit + Combat).
  * Recruit start waits for the new-turn resource reset, fixing start_gold=0.
  * Numeric SHOW_ENTITY/FULL_ENTITY/HIDE_ENTITY reconstruct hidden refresh candidates correctly.
  * ShopUpdated publishes only stable Bob PLAY state; Recruit end uses last stable revision.
  * Lobby snapshots contain all observed player HP/armor/tier/place/alive state.
  * TIMEOUT, hero power, refresh/upgrade costs and explicit frozen state are projected.
  * HearthDb supplies names, text, mechanics, tribes, tier, golden fallback and static Activate metadata.
  * Recruit actions are anchored by GameState.SendOption, independent of Power block depth.
  * Activate requires static HearthDb keyword + live INTERACTABLE_OBJECT state.
  * Final placement is logical PLAYER_ID state; results-screen Hero clones cannot override it.
  * Available tribes are a separate optional HearthMirror provider, with manual
    --tribes fallback; Power.log parsing remains independent.
"#
    );
}
