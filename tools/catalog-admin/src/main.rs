use std::{env, fs, path::PathBuf, process};

use control_protocol::{CatalogGameList, GameMetadata, UpdateGameMetadataRequest};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

enum Command {
    List,
    Show {
        game_id: String,
    },
    Set {
        game_id: String,
        edits: Vec<MetadataEdit>,
    },
    Export {
        output: Option<PathBuf>,
    },
    Import {
        input: PathBuf,
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
        Command::Show { game_id } => show_metadata(&client, &config.control_plane_url, &game_id)?,
        Command::Set { game_id, edits } => {
            update_metadata(&client, &config.control_plane_url, &game_id, &edits)?
        }
        Command::Export { output } => export_metadata(&client, &config.control_plane_url, output)?,
        Command::Import { input } => import_metadata(&client, &config.control_plane_url, input)?,
    }
    Ok(())
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
        return Err("missing command: list, show, set, export, or import".to_owned());
    };
    match command {
        "list" => Ok(Command::List),
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
        "export" => Ok(Command::Export {
            output: parse_export_output(&values[1..])?,
        }),
        "import" => Ok(Command::Import {
            input: parse_import_input(&values[1..])?,
        }),
        _ => Err(format!("unknown command: {command}")),
    }
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

fn usage() -> ! {
    eprintln!(
        "Usage:
  catalog-admin --control-plane <url> [--api-token <token>] list
  catalog-admin --control-plane <url> [--api-token <token>] show <game-id>
  catalog-admin --control-plane <url> [--api-token <token>] set <game-id> [metadata options]
  catalog-admin --control-plane <url> [--api-token <token>] export [--output <path>]
  catalog-admin --control-plane <url> [--api-token <token>] import --input <path>

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
