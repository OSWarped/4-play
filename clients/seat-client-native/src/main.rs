use std::{env, error::Error, path::PathBuf};

use seat_client_native::{
    BlockingControlPlaneClient, ControlPlaneApi, NativeSeatConfigFile, NativeSeatConfigStore,
    build_native_seat_view_model,
};

fn main() -> Result<(), Box<dyn Error>> {
    let args = env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("init-config") => init_config(&args[2..]),
        Some("snapshot") => snapshot(&args[2..]),
        Some("-h") | Some("--help") | None => {
            print_usage(&args[0]);
            Ok(())
        }
        Some(command) => Err(format!("unknown command '{command}'").into()),
    }
}

fn init_config(args: &[String]) -> Result<(), Box<dyn Error>> {
    let options = NativeCliOptions::parse(args)?;
    let store = NativeSeatConfigStore::new(required(options.config, "--config")?);
    let config = NativeSeatConfigFile {
        control_plane_url: required(options.control_plane_url, "--control-plane")?,
        seat_id: required(options.seat_id, "--seat-id")?,
        destination_address: required(options.destination_address, "--destination-ip")?,
        ffplay_path: PathBuf::from(
            options
                .ffplay_path
                .unwrap_or_else(|| default_ffplay_path().to_owned()),
        ),
    };
    store.save_file(&config)?;
    println!(
        "Saved token-safe native seat config to {}",
        store.path().display()
    );
    Ok(())
}

fn snapshot(args: &[String]) -> Result<(), Box<dyn Error>> {
    let options = NativeCliOptions::parse(args)?;
    let api_token = options
        .api_token
        .or_else(|| env::var("FOURPLAY_SEAT_API_TOKEN").ok())
        .ok_or("missing --api-token <token> or FOURPLAY_SEAT_API_TOKEN")?;
    let store = NativeSeatConfigStore::new(required(options.config, "--config")?);
    let config = store.load_with_token(api_token.clone())?;
    let mut api = BlockingControlPlaneClient::new(&config.control_plane_url, api_token);
    let games = api.list_games()?.games;
    let active_sessions = api.list_active_sessions()?.sessions;
    let view_model = build_native_seat_view_model(
        seat_client_native::NativeSeatSnapshot {
            games,
            active_sessions,
        },
        None,
        &config.seat_id,
    );
    println!("{}", serde_json::to_string_pretty(&view_model)?);
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct NativeCliOptions {
    config: Option<String>,
    control_plane_url: Option<String>,
    seat_id: Option<String>,
    destination_address: Option<String>,
    ffplay_path: Option<String>,
    api_token: Option<String>,
}

impl NativeCliOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut index = 0;
        while index < args.len() {
            let option = &args[index];
            index += 1;
            match option.as_str() {
                "--config" => options.config = Some(value(args, &mut index, option)?),
                "--control-plane" => {
                    options.control_plane_url = Some(value(args, &mut index, option)?);
                }
                "--seat-id" => options.seat_id = Some(value(args, &mut index, option)?),
                "--destination-ip" => {
                    options.destination_address = Some(value(args, &mut index, option)?);
                }
                "--ffplay-path" => options.ffplay_path = Some(value(args, &mut index, option)?),
                "--api-token" => options.api_token = Some(value(args, &mut index, option)?),
                "-h" | "--help" => return Err("help requested".to_owned()),
                _ => return Err(format!("unknown option '{option}'")),
            }
        }
        Ok(options)
    }
}

fn value(args: &[String], index: &mut usize, option: &str) -> Result<String, String> {
    let Some(value) = args.get(*index) else {
        return Err(format!("missing value for {option}"));
    };
    *index += 1;
    Ok(value.clone())
}

fn required(value: Option<String>, option: &str) -> Result<String, Box<dyn Error>> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("missing {option}").into())
}

fn default_ffplay_path() -> &'static str {
    if cfg!(windows) {
        "ffplay.exe"
    } else {
        "ffplay"
    }
}

fn print_usage(program: &str) {
    println!(
        "Usage:\n  {program} init-config --config <path> --control-plane <url> --seat-id <id> --destination-ip <seat-ip> [--ffplay-path <path>]\n  {program} snapshot --config <path> [--api-token <token>]\n\nEnvironment:\n  FOURPLAY_SEAT_API_TOKEN     Seat control-plane bearer token for snapshot."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_init_config_options() {
        let options = NativeCliOptions::parse(&[
            "--config".to_owned(),
            "seat.json".to_owned(),
            "--control-plane".to_owned(),
            "http://192.0.2.68:8080".to_owned(),
            "--seat-id".to_owned(),
            "windows-seat-1".to_owned(),
            "--destination-ip".to_owned(),
            "192.0.2.10".to_owned(),
            "--ffplay-path".to_owned(),
            "ffplay-custom".to_owned(),
        ])
        .unwrap();

        assert_eq!(options.config, Some("seat.json".to_owned()));
        assert_eq!(options.ffplay_path, Some("ffplay-custom".to_owned()));
    }

    #[test]
    fn rejects_unknown_options() {
        assert_eq!(
            NativeCliOptions::parse(&["--wat".to_owned()]),
            Err("unknown option '--wat'".to_owned())
        );
    }
}
