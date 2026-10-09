use std::{env, fs, path::PathBuf, process};

use control_protocol::{
    CatalogGame, CatalogGameList, GameMetadata, GamePlayerSlotMetadata, UpdateGameMetadataRequest,
};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

enum Command {
    List,
    Report,
    Show {
        game_id: String,
    },
    Set {
        game_id: String,
        edits: Vec<MetadataEdit>,
    },
    SetSlot {
        game_id: String,
        player_number: u32,
        edits: Vec<PlayerSlotEdit>,
    },
    Export {
        output: Option<PathBuf>,
    },
    Import {
        input: PathBuf,
    },
    ValidateAssets,
    SeedPlaceholders {
        asset_root: PathBuf,
        update_metadata: bool,
        overwrite: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MetadataEdit {
    SortTitle(Option<String>),
    Description(Option<String>),
    Genre(Option<String>),
    ReleaseYear(Option<u16>),
    Manufacturer(Option<String>),
    PlayerCount(Option<u32>),
    ArtworkPath(Option<String>),
    MarqueePath(Option<String>),
    ScreenshotPath(Option<String>),
    LogoPath(Option<String>),
    ControlNotes(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlayerSlotEdit {
    Label(Option<String>),
    Position(Option<String>),
    Character(Option<String>),
    ArtworkPath(Option<String>),
    Remove,
}

struct Config {
    control_plane_url: String,
    api_token: String,
    command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MetadataExport {
    games: Vec<MetadataExportGame>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MetadataExportGame {
    id: String,
    display_name: Option<String>,
    rom_name: Option<String>,
    metadata: GameMetadata,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = parse_args(env::args().collect()).unwrap_or_else(|error| {
        eprintln!("Error: {error}\n");
        usage();
    });
    let client = client(&config.api_token)?;
    match config.command {
        Command::List => list_games(&client, &config.control_plane_url)?,
        Command::Report => report_library(&client, &config.control_plane_url)?,
        Command::Show { game_id } => show_metadata(&client, &config.control_plane_url, &game_id)?,
        Command::Set { game_id, edits } => {
            update_metadata(&client, &config.control_plane_url, &game_id, &edits)?
        }
        Command::SetSlot {
            game_id,
            player_number,
            edits,
        } => update_slot_metadata(
            &client,
            &config.control_plane_url,
            &game_id,
            player_number,
            &edits,
        )?,
        Command::Export { output } => export_metadata(&client, &config.control_plane_url, output)?,
        Command::Import { input } => import_metadata(&client, &config.control_plane_url, input)?,
        Command::ValidateAssets => validate_assets(&client, &config.control_plane_url)?,
        Command::SeedPlaceholders {
            asset_root,
            update_metadata,
            overwrite,
        } => seed_placeholders(
            &client,
            &config.control_plane_url,
            asset_root,
            update_metadata,
            overwrite,
        )?,
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MetadataReport {
    game_id: String,
    display_name: String,
    present: usize,
    total: usize,
    missing: Vec<String>,
}

fn client(api_token: &str) -> Result<Client, Box<dyn std::error::Error>> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {api_token}"))?,
    );
    Ok(Client::builder().default_headers(headers).build()?)
}

fn list_games(client: &Client, control_plane_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let catalog = client
        .get(format!("{control_plane_url}/api/v1/games"))
        .send()?
        .error_for_status()?
        .json::<CatalogGameList>()?;
    for game in catalog.games {
        println!("{} - {} ({})", game.id, game.display_name, game.rom_name);
        if let Some(player_count) = game.metadata.player_count {
            println!("  players: {player_count}");
        }
        if let Some(genre) = game.metadata.genre {
            println!("  genre: {genre}");
        }
        if let Some(marquee) = game.metadata.marquee_path {
            println!("  marquee: {marquee}");
        }
    }
    Ok(())
}

fn report_library(
    client: &Client,
    control_plane_url: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let catalog = client
        .get(format!("{control_plane_url}/api/v1/games"))
        .send()?
        .error_for_status()?
        .json::<CatalogGameList>()?;
    let reports: Vec<_> = catalog.games.iter().map(metadata_report).collect();
    let complete = reports
        .iter()
        .filter(|report| report.missing.is_empty())
        .count();
    println!("Library metadata report");
    println!("  games: {}", reports.len());
    println!("  complete: {complete}");
    println!("  incomplete: {}", reports.len().saturating_sub(complete));
    if reports.is_empty() {
        return Ok(());
    }
    println!();
    for report in reports {
        println!(
            "{} - {}: {}/{}",
            report.game_id, report.display_name, report.present, report.total
        );
        if report.missing.is_empty() {
            println!("  complete");
        } else {
            println!("  missing: {}", report.missing.join(", "));
        }
    }
    Ok(())
}

fn metadata_report(game: &CatalogGame) -> MetadataReport {
    let mut items = vec![
        ("sort_title".to_owned(), has_text(&game.metadata.sort_title)),
        (
            "description".to_owned(),
            has_text(&game.metadata.description),
        ),
        ("genre".to_owned(), has_text(&game.metadata.genre)),
        (
            "release_year".to_owned(),
            game.metadata.release_year.is_some(),
        ),
        (
            "manufacturer".to_owned(),
            has_text(&game.metadata.manufacturer),
        ),
        (
            "player_count".to_owned(),
            effective_player_count(game).is_some(),
        ),
        (
            "artwork_path".to_owned(),
            has_text(&game.metadata.artwork_path),
        ),
        (
            "marquee_path".to_owned(),
            has_text(&game.metadata.marquee_path),
        ),
        (
            "screenshot_path".to_owned(),
            has_text(&game.metadata.screenshot_path),
        ),
        ("logo_path".to_owned(), has_text(&game.metadata.logo_path)),
        (
            "control_notes".to_owned(),
            has_text(&game.metadata.control_notes),
        ),
    ];
    if effective_player_count(game).is_some_and(|count| count > 1) {
        items.push((
            "player_slot_labels".to_owned(),
            player_slot_labels_complete(game),
        ));
    }

    let total = items.len();
    let present = items.iter().filter(|(_, present)| *present).count();
    let missing = items
        .into_iter()
        .filter_map(|(field, present)| (!present).then_some(field))
        .collect();

    MetadataReport {
        game_id: game.id.clone(),
        display_name: game.display_name.clone(),
        present,
        total,
        missing,
    }
}

fn has_text(value: &Option<String>) -> bool {
    value
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
}

fn effective_player_count(game: &CatalogGame) -> Option<u32> {
    game.metadata.player_count.or_else(|| {
        game.availability
            .iter()
            .map(|availability| availability.profile.max_players)
            .max()
    })
}

fn player_slot_labels_complete(game: &CatalogGame) -> bool {
    let Some(player_count) = effective_player_count(game) else {
        return false;
    };
    (1..=player_count).all(|player_number| {
        game.metadata.player_slots.iter().any(|slot| {
            slot.player_number == player_number
                && slot
                    .label
                    .as_deref()
                    .is_some_and(|label| !label.trim().is_empty())
        })
    })
}

fn show_metadata(
    client: &Client,
    control_plane_url: &str,
    game_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let metadata = fetch_metadata(client, control_plane_url, game_id)?;
    println!("{}", serde_json::to_string_pretty(&metadata)?);
    Ok(())
}

fn update_metadata(
    client: &Client,
    control_plane_url: &str,
    game_id: &str,
    edits: &[MetadataEdit],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut metadata = fetch_metadata(client, control_plane_url, game_id)?;
    apply_edits(&mut metadata, edits);
    let game = client
        .put(format!(
            "{control_plane_url}/api/v1/games/{game_id}/metadata"
        ))
        .json(&UpdateGameMetadataRequest { metadata })
        .send()?
        .error_for_status()?
        .json::<serde_json::Value>()?;
    println!("{}", serde_json::to_string_pretty(&game)?);
    Ok(())
}

fn update_slot_metadata(
    client: &Client,
    control_plane_url: &str,
    game_id: &str,
    player_number: u32,
    edits: &[PlayerSlotEdit],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut metadata = fetch_metadata(client, control_plane_url, game_id)?;
    apply_slot_edits(&mut metadata, player_number, edits);
    let game = client
        .put(format!(
            "{control_plane_url}/api/v1/games/{game_id}/metadata"
        ))
        .json(&UpdateGameMetadataRequest { metadata })
        .send()?
        .error_for_status()?
        .json::<serde_json::Value>()?;
    println!("{}", serde_json::to_string_pretty(&game)?);
    Ok(())
}

fn export_metadata(
    client: &Client,
    control_plane_url: &str,
    output: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let catalog = client
        .get(format!("{control_plane_url}/api/v1/games"))
        .send()?
        .error_for_status()?
        .json::<CatalogGameList>()?;
    let export = MetadataExport {
        games: catalog
            .games
            .into_iter()
            .map(|game| MetadataExportGame {
                id: game.id,
                display_name: Some(game.display_name),
                rom_name: Some(game.rom_name),
                metadata: game.metadata,
            })
            .collect(),
    };
    let json = serde_json::to_string_pretty(&export)?;
    if let Some(path) = output {
        fs::write(&path, format!("{json}\n"))?;
        println!(
            "Exported metadata for {} games to {}",
            export.games.len(),
            path.display()
        );
    } else {
        println!("{json}");
    }
    Ok(())
}

fn import_metadata(
    client: &Client,
    control_plane_url: &str,
    input: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let raw = fs::read_to_string(&input)?;
    let export: MetadataExport = serde_json::from_str(&raw)?;
    let mut updated = 0usize;
    for game in export.games {
        client
            .put(format!(
                "{control_plane_url}/api/v1/games/{}/metadata",
                game.id
            ))
            .json(&UpdateGameMetadataRequest {
                metadata: game.metadata,
            })
            .send()?
            .error_for_status()?;
        updated += 1;
    }
    println!(
        "Imported metadata for {updated} games from {}",
        input.display()
    );
    Ok(())
}

fn validate_assets(
    client: &Client,
    control_plane_url: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let catalog = client
        .get(format!("{control_plane_url}/api/v1/games"))
        .send()?
        .error_for_status()?
        .json::<CatalogGameList>()?;
    let mut checked = 0usize;
    let mut missing = Vec::new();
    for game in catalog.games {
        for (field, asset_path) in asset_references(&game.metadata) {
            checked += 1;
            let url = format!(
                "{control_plane_url}/api/v1/assets/{}",
                percent_encode_asset_path(asset_path)
            );
            let response = client.get(url).send()?;
            if response.status().is_success() {
                println!("ok      {} {field} {asset_path}", game.id);
            } else {
                println!(
                    "missing {} {field} {asset_path} ({})",
                    game.id,
                    response.status()
                );
                missing.push(format!("{} {field} {asset_path}", game.id));
            }
        }
    }
    if checked == 0 {
        println!("No metadata asset paths to validate.");
        return Ok(());
    }
    if missing.is_empty() {
        println!(
            "Validated {checked} metadata asset path{}.",
            plural(checked)
        );
        Ok(())
    } else {
        Err(format!(
            "{} of {checked} metadata asset path{} failed validation",
            missing.len(),
            plural(checked)
        )
        .into())
    }
}

fn seed_placeholders(
    client: &Client,
    control_plane_url: &str,
    asset_root: PathBuf,
    update_metadata: bool,
    overwrite: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let catalog = client
        .get(format!("{control_plane_url}/api/v1/games"))
        .send()?
        .error_for_status()?
        .json::<CatalogGameList>()?;
    let game_count = catalog.games.len();
    let mut written = 0usize;
    let mut skipped = 0usize;
    let mut metadata_updates = 0usize;
    for game in catalog.games {
        let paths = conventional_asset_paths(&game.id);
        let media_dir = asset_root.join("media").join(&game.id);
        fs::create_dir_all(&media_dir)?;
        for (kind, relative_path) in &paths {
            let full_path = asset_root.join(relative_path);
            if full_path.exists() && !overwrite {
                skipped += 1;
                continue;
            }
            let svg = placeholder_svg(&game, kind);
            fs::write(&full_path, svg)?;
            written += 1;
        }
        if update_metadata {
            let mut metadata = game.metadata;
            apply_placeholder_paths(&mut metadata, &paths, overwrite);
            client
                .put(format!(
                    "{control_plane_url}/api/v1/games/{}/metadata",
                    game.id
                ))
                .json(&UpdateGameMetadataRequest { metadata })
                .send()?
                .error_for_status()?;
            metadata_updates += 1;
        }
    }
    println!(
        "Seeded placeholder assets for {game_count} game{} in {}: {written} written, {skipped} skipped.",
        plural(game_count),
        asset_root.display()
    );
    if update_metadata {
        println!(
            "Updated metadata paths for {metadata_updates} game{}.",
            plural(metadata_updates)
        );
    } else {
        println!(
            "Metadata was not changed. Pass --update-metadata to point games at the placeholder assets."
        );
    }
    Ok(())
}

fn conventional_asset_paths(game_id: &str) -> [(&'static str, String); 4] {
    [
        ("artwork", format!("media/{game_id}/artwork.svg")),
        ("marquee", format!("media/{game_id}/marquee.svg")),
        ("screenshot", format!("media/{game_id}/screenshot.svg")),
        ("logo", format!("media/{game_id}/logo.svg")),
    ]
}

fn apply_placeholder_paths(
    metadata: &mut GameMetadata,
    paths: &[(&'static str, String); 4],
    overwrite: bool,
) {
    for (kind, path) in paths {
        match *kind {
            "artwork" if overwrite || metadata.artwork_path.is_none() => {
                metadata.artwork_path = Some(path.clone())
            }
            "marquee" if overwrite || metadata.marquee_path.is_none() => {
                metadata.marquee_path = Some(path.clone())
            }
            "screenshot" if overwrite || metadata.screenshot_path.is_none() => {
                metadata.screenshot_path = Some(path.clone())
            }
            "logo" if overwrite || metadata.logo_path.is_none() => {
                metadata.logo_path = Some(path.clone())
            }
            _ => {}
        }
    }
}

fn placeholder_svg(game: &CatalogGame, kind: &str) -> String {
    let title = escape_xml(&game.display_name);
    let game_id = escape_xml(&game.id);
    let label = escape_xml(&kind.to_ascii_uppercase());
    let (width, height, background) = match kind {
        "marquee" => (640, 160, "#141827"),
        "screenshot" => (640, 448, "#111827"),
        "logo" => (512, 256, "#0f172a"),
        _ => (512, 512, "#1f2937"),
    };
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
  <rect width="100%" height="100%" fill="{background}"/>
  <rect x="16" y="16" width="{inner_width}" height="{inner_height}" rx="18" fill="none" stroke="#38bdf8" stroke-width="4"/>
  <text x="50%" y="42%" dominant-baseline="middle" text-anchor="middle" font-family="Arial, Helvetica, sans-serif" font-size="{title_size}" font-weight="700" fill="#f8fafc">{title}</text>
  <text x="50%" y="60%" dominant-baseline="middle" text-anchor="middle" font-family="Arial, Helvetica, sans-serif" font-size="{label_size}" fill="#93c5fd">4-Play placeholder {label}</text>
  <text x="50%" y="76%" dominant-baseline="middle" text-anchor="middle" font-family="Arial, Helvetica, sans-serif" font-size="20" fill="#64748b">{game_id}</text>
</svg>
"##,
        inner_width = width - 32,
        inner_height = height - 32,
        title_size = if width >= 640 { 36 } else { 30 },
        label_size = if width >= 640 { 24 } else { 20 },
    )
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn asset_references(metadata: &GameMetadata) -> Vec<(&'static str, &str)> {
    let mut references = Vec::new();
    if let Some(path) = metadata.artwork_path.as_deref() {
        references.push(("artwork_path", path));
    }
    if let Some(path) = metadata.marquee_path.as_deref() {
        references.push(("marquee_path", path));
    }
    if let Some(path) = metadata.screenshot_path.as_deref() {
        references.push(("screenshot_path", path));
    }
    if let Some(path) = metadata.logo_path.as_deref() {
        references.push(("logo_path", path));
    }
    for slot in &metadata.player_slots {
        if let Some(path) = slot.artwork_path.as_deref() {
            references.push(("player_slot.artwork_path", path));
        }
    }
    references
}

fn percent_encode_asset_path(path: &str) -> String {
    let mut encoded = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn fetch_metadata(
    client: &Client,
    control_plane_url: &str,
    game_id: &str,
) -> Result<GameMetadata, Box<dyn std::error::Error>> {
    Ok(client
        .get(format!(
            "{control_plane_url}/api/v1/games/{game_id}/metadata"
        ))
        .send()?
        .error_for_status()?
        .json()?)
}

fn parse_args(args: Vec<String>) -> Result<Config, String> {
    let mut control_plane_url = env::var("FOURPLAY_CONTROL_PLANE_URL").ok();
    let mut api_token = env::var("FOURPLAY_ADMIN_API_TOKEN")
        .ok()
        .or_else(|| env::var("FOURPLAY_SEAT_API_TOKEN").ok());
    let mut rest = Vec::new();
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--control-plane" => {
                index += 1;
                control_plane_url = Some(
                    args.get(index)
                        .cloned()
                        .ok_or("--control-plane requires a value")?,
                );
            }
            "--api-token" => {
                index += 1;
                api_token = Some(
                    args.get(index)
                        .cloned()
                        .ok_or("--api-token requires a value")?,
                );
            }
            "--help" | "-h" => usage(),
            value => rest.push(value.to_owned()),
        }
        index += 1;
    }
    let control_plane_url = control_plane_url
        .ok_or("missing --control-plane <url> or FOURPLAY_CONTROL_PLANE_URL")?
        .trim_end_matches('/')
        .to_owned();
    let api_token = api_token.ok_or("missing --api-token <token> or FOURPLAY_ADMIN_API_TOKEN")?;
    if api_token.len() < 16 {
        return Err("API token must contain at least 16 characters".to_owned());
    }
    let command = parse_command(&rest)?;
    Ok(Config {
        control_plane_url,
        api_token,
        command,
    })
}

fn parse_command(values: &[String]) -> Result<Command, String> {
    let Some(command) = values.first().map(String::as_str) else {
        return Err(
            "missing command: list, report, show, set, set-slot, export, import, validate-assets, or seed-placeholders"
                .to_owned(),
        );
    };
    match command {
        "list" => Ok(Command::List),
        "report" => {
            if values.len() > 1 {
                return Err("report does not accept options".to_owned());
            }
            Ok(Command::Report)
        }
        "show" => Ok(Command::Show {
            game_id: values.get(1).cloned().ok_or("show requires a game id")?,
        }),
        "set" => {
            let game_id = values.get(1).cloned().ok_or("set requires a game id")?;
            let edits = parse_edits(&values[2..])?;
            if edits.is_empty() {
                return Err("set requires at least one metadata field".to_owned());
            }
            Ok(Command::Set { game_id, edits })
        }
        "set-slot" => {
            let game_id = values
                .get(1)
                .cloned()
                .ok_or("set-slot requires a game id")?;
            let player_number = values
                .get(2)
                .ok_or("set-slot requires a player number")?
                .parse::<u32>()
                .map_err(|error| format!("invalid set-slot player number: {error}"))?;
            let edits = parse_slot_edits(&values[3..])?;
            if edits.is_empty() {
                return Err("set-slot requires at least one slot option".to_owned());
            }
            Ok(Command::SetSlot {
                game_id,
                player_number,
                edits,
            })
        }
        "export" => Ok(Command::Export {
            output: parse_export_output(&values[1..])?,
        }),
        "import" => Ok(Command::Import {
            input: parse_import_input(&values[1..])?,
        }),
        "validate-assets" => {
            if values.len() > 1 {
                return Err("validate-assets does not accept options".to_owned());
            }
            Ok(Command::ValidateAssets)
        }
        "seed-placeholders" => parse_seed_placeholders(&values[1..]),
        _ => Err(format!("unknown command: {command}")),
    }
}

fn parse_slot_edits(values: &[String]) -> Result<Vec<PlayerSlotEdit>, String> {
    let mut edits = Vec::new();
    let mut index = 0;
    while index < values.len() {
        let option = values[index].as_str();
        if option == "--remove" {
            edits.push(PlayerSlotEdit::Remove);
            index += 1;
            continue;
        }
        if option == "--clear" {
            index += 1;
            let field = values
                .get(index)
                .ok_or("--clear requires a slot field name")?;
            edits.push(clear_slot_edit(field)?);
            index += 1;
            continue;
        }
        index += 1;
        let raw_value = values
            .get(index)
            .cloned()
            .ok_or_else(|| format!("{option} requires a value"))?;
        edits.push(match option {
            "--label" => PlayerSlotEdit::Label(Some(raw_value)),
            "--position" => PlayerSlotEdit::Position(Some(raw_value)),
            "--character" => PlayerSlotEdit::Character(Some(raw_value)),
            "--artwork-path" => PlayerSlotEdit::ArtworkPath(Some(raw_value)),
            _ => return Err(format!("unknown set-slot option: {option}")),
        });
        index += 1;
    }
    Ok(edits)
}

fn clear_slot_edit(field: &str) -> Result<PlayerSlotEdit, String> {
    match field {
        "label" => Ok(PlayerSlotEdit::Label(None)),
        "position" => Ok(PlayerSlotEdit::Position(None)),
        "character" => Ok(PlayerSlotEdit::Character(None)),
        "artwork-path" => Ok(PlayerSlotEdit::ArtworkPath(None)),
        _ => Err(format!("unknown slot clear field: {field}")),
    }
}

fn parse_seed_placeholders(values: &[String]) -> Result<Command, String> {
    let mut asset_root = None;
    let mut update_metadata = false;
    let mut overwrite = false;
    let mut index = 0;
    while index < values.len() {
        match values[index].as_str() {
            "--asset-root" => {
                index += 1;
                let path = values.get(index).ok_or("--asset-root requires a path")?;
                asset_root = Some(PathBuf::from(path));
            }
            "--update-metadata" => update_metadata = true,
            "--overwrite" => overwrite = true,
            option => return Err(format!("unknown seed-placeholders option: {option}")),
        }
        index += 1;
    }
    Ok(Command::SeedPlaceholders {
        asset_root: asset_root.ok_or("seed-placeholders requires --asset-root <path>")?,
        update_metadata,
        overwrite,
    })
}

fn parse_export_output(values: &[String]) -> Result<Option<PathBuf>, String> {
    let mut output = None;
    let mut index = 0;
    while index < values.len() {
        match values[index].as_str() {
            "--output" => {
                index += 1;
                let path = values.get(index).ok_or("--output requires a path")?;
                output = Some(PathBuf::from(path));
            }
            option => return Err(format!("unknown export option: {option}")),
        }
        index += 1;
    }
    Ok(output)
}

fn parse_import_input(values: &[String]) -> Result<PathBuf, String> {
    let mut input = None;
    let mut index = 0;
    while index < values.len() {
        match values[index].as_str() {
            "--input" => {
                index += 1;
                let path = values.get(index).ok_or("--input requires a path")?;
                input = Some(PathBuf::from(path));
            }
            option => return Err(format!("unknown import option: {option}")),
        }
        index += 1;
    }
    input.ok_or("import requires --input <path>".to_owned())
}

fn parse_edits(values: &[String]) -> Result<Vec<MetadataEdit>, String> {
    let mut edits = Vec::new();
    let mut index = 0;
    while index < values.len() {
        let option = values[index].as_str();
        if option == "--clear" {
            index += 1;
            let field = values.get(index).ok_or("--clear requires a field name")?;
            edits.push(clear_edit(field)?);
            index += 1;
            continue;
        }
        index += 1;
        let raw_value = values
            .get(index)
            .cloned()
            .ok_or_else(|| format!("{option} requires a value"))?;
        edits.push(match option {
            "--sort-title" => MetadataEdit::SortTitle(Some(raw_value)),
            "--description" => MetadataEdit::Description(Some(raw_value)),
            "--genre" => MetadataEdit::Genre(Some(raw_value)),
            "--release-year" => MetadataEdit::ReleaseYear(Some(
                raw_value
                    .parse::<u16>()
                    .map_err(|error| format!("invalid --release-year: {error}"))?,
            )),
            "--manufacturer" => MetadataEdit::Manufacturer(Some(raw_value)),
            "--player-count" => MetadataEdit::PlayerCount(Some(
                raw_value
                    .parse::<u32>()
                    .map_err(|error| format!("invalid --player-count: {error}"))?,
            )),
            "--artwork-path" => MetadataEdit::ArtworkPath(Some(raw_value)),
            "--marquee-path" => MetadataEdit::MarqueePath(Some(raw_value)),
            "--screenshot-path" => MetadataEdit::ScreenshotPath(Some(raw_value)),
            "--logo-path" => MetadataEdit::LogoPath(Some(raw_value)),
            "--control-notes" => MetadataEdit::ControlNotes(Some(raw_value)),
            _ => return Err(format!("unknown metadata option: {option}")),
        });
        index += 1;
    }
    Ok(edits)
}

fn clear_edit(field: &str) -> Result<MetadataEdit, String> {
    match field {
        "sort-title" => Ok(MetadataEdit::SortTitle(None)),
        "description" => Ok(MetadataEdit::Description(None)),
        "genre" => Ok(MetadataEdit::Genre(None)),
        "release-year" => Ok(MetadataEdit::ReleaseYear(None)),
        "manufacturer" => Ok(MetadataEdit::Manufacturer(None)),
        "player-count" => Ok(MetadataEdit::PlayerCount(None)),
        "artwork-path" => Ok(MetadataEdit::ArtworkPath(None)),
        "marquee-path" => Ok(MetadataEdit::MarqueePath(None)),
        "screenshot-path" => Ok(MetadataEdit::ScreenshotPath(None)),
        "logo-path" => Ok(MetadataEdit::LogoPath(None)),
        "control-notes" => Ok(MetadataEdit::ControlNotes(None)),
        _ => Err(format!("unknown clear field: {field}")),
    }
}

fn apply_edits(metadata: &mut GameMetadata, edits: &[MetadataEdit]) {
    for edit in edits {
        match edit {
            MetadataEdit::SortTitle(value) => metadata.sort_title = value.clone(),
            MetadataEdit::Description(value) => metadata.description = value.clone(),
            MetadataEdit::Genre(value) => metadata.genre = value.clone(),
            MetadataEdit::ReleaseYear(value) => metadata.release_year = *value,
            MetadataEdit::Manufacturer(value) => metadata.manufacturer = value.clone(),
            MetadataEdit::PlayerCount(value) => metadata.player_count = *value,
            MetadataEdit::ArtworkPath(value) => metadata.artwork_path = value.clone(),
            MetadataEdit::MarqueePath(value) => metadata.marquee_path = value.clone(),
            MetadataEdit::ScreenshotPath(value) => metadata.screenshot_path = value.clone(),
            MetadataEdit::LogoPath(value) => metadata.logo_path = value.clone(),
            MetadataEdit::ControlNotes(value) => metadata.control_notes = value.clone(),
        }
    }
}

fn apply_slot_edits(metadata: &mut GameMetadata, player_number: u32, edits: &[PlayerSlotEdit]) {
    if edits
        .iter()
        .any(|edit| matches!(edit, PlayerSlotEdit::Remove))
    {
        metadata
            .player_slots
            .retain(|slot| slot.player_number != player_number);
        return;
    }
    let index = metadata
        .player_slots
        .iter()
        .position(|slot| slot.player_number == player_number);
    if index.is_none() {
        metadata.player_slots.push(GamePlayerSlotMetadata {
            player_number,
            label: None,
            position: None,
            character: None,
            artwork_path: None,
        });
    }
    let slot = metadata
        .player_slots
        .iter_mut()
        .find(|slot| slot.player_number == player_number)
        .expect("slot metadata was just inserted when missing");
    for edit in edits {
        match edit {
            PlayerSlotEdit::Label(value) => slot.label = value.clone(),
            PlayerSlotEdit::Position(value) => slot.position = value.clone(),
            PlayerSlotEdit::Character(value) => slot.character = value.clone(),
            PlayerSlotEdit::ArtworkPath(value) => slot.artwork_path = value.clone(),
            PlayerSlotEdit::Remove => {}
        }
    }
}

fn usage() -> ! {
    eprintln!(
        "Usage:
  catalog-admin --control-plane <url> [--api-token <token>] list
  catalog-admin --control-plane <url> [--api-token <token>] report
  catalog-admin --control-plane <url> [--api-token <token>] show <game-id>
  catalog-admin --control-plane <url> [--api-token <token>] set <game-id> [metadata options]
  catalog-admin --control-plane <url> [--api-token <token>] set-slot <game-id> <player-number> [slot options]
  catalog-admin --control-plane <url> [--api-token <token>] export [--output <path>]
  catalog-admin --control-plane <url> [--api-token <token>] import --input <path>
  catalog-admin --control-plane <url> [--api-token <token>] validate-assets
  catalog-admin --control-plane <url> [--api-token <token>] seed-placeholders --asset-root <path> [--update-metadata] [--overwrite]

Metadata options:
  --sort-title <text>
  --description <text>
  --genre <text>
  --release-year <year>
  --manufacturer <text>
  --player-count <count>
  --artwork-path <relative-path>
  --marquee-path <relative-path>
  --screenshot-path <relative-path>
  --logo-path <relative-path>
  --control-notes <text>
  --clear <field-name>

Slot options:
  --label <text>
  --position <text>
  --character <text>
  --artwork-path <relative-path>
  --clear <label|position|character|artwork-path>
  --remove

Environment:
  FOURPLAY_CONTROL_PLANE_URL
  FOURPLAY_ADMIN_API_TOKEN, falling back to FOURPLAY_SEAT_API_TOKEN"
    );
    process::exit(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_metadata_edits() {
        let edits = parse_edits(&[
            "--player-count".to_owned(),
            "4".to_owned(),
            "--marquee-path".to_owned(),
            "media/tmnt/marquee.png".to_owned(),
        ])
        .unwrap();

        assert_eq!(
            edits,
            vec![
                MetadataEdit::PlayerCount(Some(4)),
                MetadataEdit::MarqueePath(Some("media/tmnt/marquee.png".to_owned()))
            ]
        );
    }

    #[test]
    fn applies_and_clears_metadata_edits() {
        let mut metadata = GameMetadata::default();
        apply_edits(
            &mut metadata,
            &[
                MetadataEdit::Genre(Some("Beat 'em up".to_owned())),
                MetadataEdit::PlayerCount(Some(4)),
                MetadataEdit::PlayerCount(None),
            ],
        );

        assert_eq!(metadata.genre.as_deref(), Some("Beat 'em up"));
        assert_eq!(metadata.player_count, None);
    }

    #[test]
    fn rejects_unknown_clear_fields() {
        assert_eq!(
            clear_edit("unknown").unwrap_err(),
            "unknown clear field: unknown"
        );
    }

    #[test]
    fn parses_export_output_path() {
        let command = parse_command(&[
            "export".to_owned(),
            "--output".to_owned(),
            "metadata.json".to_owned(),
        ])
        .unwrap();

        match command {
            Command::Export { output } => {
                assert_eq!(output, Some(PathBuf::from("metadata.json")));
            }
            _ => panic!("expected export command"),
        }
    }

    #[test]
    fn parses_import_input_path() {
        let command = parse_command(&[
            "import".to_owned(),
            "--input".to_owned(),
            "metadata.json".to_owned(),
        ])
        .unwrap();

        match command {
            Command::Import { input } => {
                assert_eq!(input, PathBuf::from("metadata.json"));
            }
            _ => panic!("expected import command"),
        }
    }

    #[test]
    fn parses_validate_assets_command() {
        let command = parse_command(&["validate-assets".to_owned()]).unwrap();

        match command {
            Command::ValidateAssets => {}
            _ => panic!("expected validate-assets command"),
        }
    }

    #[test]
    fn parses_report_command() {
        let command = parse_command(&["report".to_owned()]).unwrap();

        match command {
            Command::Report => {}
            _ => panic!("expected report command"),
        }
    }

    #[test]
    fn parses_set_slot_command() {
        let command = parse_command(&[
            "set-slot".to_owned(),
            "tmnt".to_owned(),
            "2".to_owned(),
            "--label".to_owned(),
            "Donatello".to_owned(),
            "--position".to_owned(),
            "P2".to_owned(),
            "--artwork-path".to_owned(),
            "media/tmnt/p2.svg".to_owned(),
        ])
        .unwrap();

        match command {
            Command::SetSlot {
                game_id,
                player_number,
                edits,
            } => {
                assert_eq!(game_id, "tmnt");
                assert_eq!(player_number, 2);
                assert_eq!(
                    edits,
                    vec![
                        PlayerSlotEdit::Label(Some("Donatello".to_owned())),
                        PlayerSlotEdit::Position(Some("P2".to_owned())),
                        PlayerSlotEdit::ArtworkPath(Some("media/tmnt/p2.svg".to_owned())),
                    ]
                );
            }
            _ => panic!("expected set-slot command"),
        }
    }

    #[test]
    fn applies_slot_edits_and_removes_slot_metadata() {
        let mut metadata = GameMetadata::default();

        apply_slot_edits(
            &mut metadata,
            2,
            &[
                PlayerSlotEdit::Label(Some("Donatello".to_owned())),
                PlayerSlotEdit::Position(Some("P2".to_owned())),
                PlayerSlotEdit::Character(Some("Donatello".to_owned())),
            ],
        );

        assert_eq!(metadata.player_slots.len(), 1);
        assert_eq!(metadata.player_slots[0].player_number, 2);
        assert_eq!(metadata.player_slots[0].label.as_deref(), Some("Donatello"));
        assert_eq!(metadata.player_slots[0].position.as_deref(), Some("P2"));

        apply_slot_edits(&mut metadata, 2, &[PlayerSlotEdit::Remove]);

        assert!(metadata.player_slots.is_empty());
    }

    #[test]
    fn parses_seed_placeholders_command() {
        let command = parse_command(&[
            "seed-placeholders".to_owned(),
            "--asset-root".to_owned(),
            "assets/cache".to_owned(),
            "--update-metadata".to_owned(),
            "--overwrite".to_owned(),
        ])
        .unwrap();

        match command {
            Command::SeedPlaceholders {
                asset_root,
                update_metadata,
                overwrite,
            } => {
                assert_eq!(asset_root, PathBuf::from("assets/cache"));
                assert!(update_metadata);
                assert!(overwrite);
            }
            _ => panic!("expected seed-placeholders command"),
        }
    }

    #[test]
    fn asset_references_list_metadata_media_paths() {
        let metadata = GameMetadata {
            artwork_path: Some("media/tmnt/artwork.png".to_owned()),
            marquee_path: Some("media/tmnt/marquee.png".to_owned()),
            screenshot_path: None,
            logo_path: Some("media/tmnt/logo.png".to_owned()),
            player_slots: vec![GamePlayerSlotMetadata {
                player_number: 2,
                label: Some("Donatello".to_owned()),
                position: Some("P2".to_owned()),
                character: Some("Donatello".to_owned()),
                artwork_path: Some("media/tmnt/p2.svg".to_owned()),
            }],
            ..GameMetadata::default()
        };

        assert_eq!(
            asset_references(&metadata),
            vec![
                ("artwork_path", "media/tmnt/artwork.png"),
                ("marquee_path", "media/tmnt/marquee.png"),
                ("logo_path", "media/tmnt/logo.png"),
                ("player_slot.artwork_path", "media/tmnt/p2.svg"),
            ]
        );
    }

    #[test]
    fn conventional_asset_paths_use_game_id_directories() {
        assert_eq!(
            conventional_asset_paths("tmnt"),
            [
                ("artwork", "media/tmnt/artwork.svg".to_owned()),
                ("marquee", "media/tmnt/marquee.svg".to_owned()),
                ("screenshot", "media/tmnt/screenshot.svg".to_owned()),
                ("logo", "media/tmnt/logo.svg".to_owned()),
            ]
        );
    }

    #[test]
    fn placeholder_paths_only_replace_existing_metadata_when_overwriting() {
        let paths = conventional_asset_paths("tmnt");
        let mut metadata = GameMetadata {
            artwork_path: Some("custom/artwork.png".to_owned()),
            ..GameMetadata::default()
        };

        apply_placeholder_paths(&mut metadata, &paths, false);

        assert_eq!(metadata.artwork_path.as_deref(), Some("custom/artwork.png"));
        assert_eq!(
            metadata.marquee_path.as_deref(),
            Some("media/tmnt/marquee.svg")
        );

        apply_placeholder_paths(&mut metadata, &paths, true);

        assert_eq!(
            metadata.artwork_path.as_deref(),
            Some("media/tmnt/artwork.svg")
        );
    }

    #[test]
    fn placeholder_svg_escapes_game_titles() {
        let game = CatalogGame {
            id: "test".to_owned(),
            display_name: "A&B <Game>".to_owned(),
            rom_name: "test".to_owned(),
            metadata: GameMetadata::default(),
            availability: Vec::new(),
        };

        let svg = placeholder_svg(&game, "marquee");

        assert!(svg.contains("A&amp;B &lt;Game&gt;"));
        assert!(svg.contains("4-Play placeholder MARQUEE"));
    }

    #[test]
    fn asset_paths_are_percent_encoded_for_urls() {
        assert_eq!(
            percent_encode_asset_path("media/TMNT marquee #1.png"),
            "media/TMNT%20marquee%20%231.png"
        );
    }

    #[test]
    fn metadata_report_lists_missing_browser_presentation_fields() {
        let game = CatalogGame {
            id: "tmnt".to_owned(),
            display_name: "Teenage Mutant Ninja Turtles".to_owned(),
            rom_name: "tmnt".to_owned(),
            metadata: GameMetadata {
                genre: Some("Beat 'em up".to_owned()),
                player_slots: vec![GamePlayerSlotMetadata {
                    player_number: 1,
                    label: Some("Leonardo".to_owned()),
                    position: None,
                    character: None,
                    artwork_path: None,
                }],
                ..GameMetadata::default()
            },
            availability: vec![control_protocol::GameAvailability {
                runtime_host_id: "reference-linux".to_owned(),
                runtime_host_status: control_protocol::RuntimeHostStatus::Online,
                profile: control_protocol::GameRuntimeProfile {
                    width: 320,
                    height: 224,
                    refresh_hz: 60.0,
                    rotation_degrees: 0,
                    max_players: 4,
                    buttons_per_player: 2,
                    supports_save_state: true,
                },
            }],
        };

        let report = metadata_report(&game);

        assert_eq!(report.present, 2);
        assert_eq!(report.total, 12);
        assert!(report.missing.contains(&"description".to_owned()));
        assert!(report.missing.contains(&"player_slot_labels".to_owned()));
        assert!(!report.missing.contains(&"player_count".to_owned()));
    }

    #[test]
    fn metadata_report_accepts_complete_browser_presentation_fields() {
        let game = CatalogGame {
            id: "aliens".to_owned(),
            display_name: "Aliens".to_owned(),
            rom_name: "aliens".to_owned(),
            metadata: GameMetadata {
                sort_title: Some("Aliens".to_owned()),
                description: Some("Arcade action.".to_owned()),
                genre: Some("Run and gun".to_owned()),
                release_year: Some(1990),
                manufacturer: Some("Konami".to_owned()),
                player_count: Some(2),
                artwork_path: Some("media/aliens/artwork.svg".to_owned()),
                marquee_path: Some("media/aliens/marquee.svg".to_owned()),
                screenshot_path: Some("media/aliens/screenshot.svg".to_owned()),
                logo_path: Some("media/aliens/logo.svg".to_owned()),
                control_notes: Some("Move, shoot, jump.".to_owned()),
                player_slots: vec![
                    GamePlayerSlotMetadata {
                        player_number: 1,
                        label: Some("P1".to_owned()),
                        position: None,
                        character: None,
                        artwork_path: None,
                    },
                    GamePlayerSlotMetadata {
                        player_number: 2,
                        label: Some("P2".to_owned()),
                        position: None,
                        character: None,
                        artwork_path: None,
                    },
                ],
            },
            availability: Vec::new(),
        };

        let report = metadata_report(&game);

        assert_eq!(report.present, report.total);
        assert!(report.missing.is_empty());
    }

    #[test]
    fn round_trips_metadata_export_payload() {
        let export = MetadataExport {
            games: vec![MetadataExportGame {
                id: "tmnt".to_owned(),
                display_name: Some("Teenage Mutant Ninja Turtles".to_owned()),
                rom_name: Some("tmnt".to_owned()),
                metadata: GameMetadata {
                    genre: Some("Beat 'em up".to_owned()),
                    player_count: Some(4),
                    ..GameMetadata::default()
                },
            }],
        };

        let json = serde_json::to_string(&export).unwrap();
        let parsed: MetadataExport = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, export);
    }
}
