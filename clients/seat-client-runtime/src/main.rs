use std::{
    env, io,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

use seat_client_core::media_receiver_plan;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeConfig {
    command: RuntimeCommand,
    ffplay_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeCommand {
    Media { port: u16 },
}

fn main() {
    let program = env::args()
        .next()
        .unwrap_or_else(|| "seat-client-runtime".to_owned());
    match parse_args(env::args().skip(1)) {
        Ok(config) => {
            if let Err(error) = run(config) {
                eprintln!("Error: {error}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("{error}\n\n{}", usage(&program));
            std::process::exit(2);
        }
    }
}

fn run(config: RuntimeConfig) -> io::Result<()> {
    match config.command {
        RuntimeCommand::Media { port } => {
            let plan = media_receiver_plan(port);
            println!("Starting 4-Play media receiver on UDP port {port}.");
            println!("Receiver URL: {}", plan.receiver_url);
            println!("Press Ctrl+C to stop the receiver.");
            let mut child = spawn_ffplay(&config.ffplay_path, &plan.ffplay_args)?;
            loop {
                if let Some(status) = child.try_wait()? {
                    println!("ffplay exited with {status}.");
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(250));
            }
        }
    }
}

fn spawn_ffplay(path: &PathBuf, args: &[String]) -> io::Result<Child> {
    Command::new(path).args(args).stdin(Stdio::null()).spawn()
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<RuntimeConfig, String> {
    let mut args = args.into_iter().peekable();
    let mut ffplay_path = env::var("FOURPLAY_FFPLAY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("ffplay"));
    let Some(command) = args.next() else {
        return Err("missing command".to_owned());
    };
    let command = match command.as_str() {
        "media" => {
            let mut port = None;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--port" => {
                        port = Some(parse_port(value(&mut args, &option)?)?);
                    }
                    "--ffplay-path" => {
                        ffplay_path = PathBuf::from(value(&mut args, &option)?);
                    }
                    "--help" | "-h" => return Err("help requested".to_owned()),
                    other => return Err(format!("unknown option {other}")),
                }
            }
            RuntimeCommand::Media {
                port: port.ok_or_else(|| "missing --port <udp-port>".to_owned())?,
            }
        }
        "--help" | "-h" => return Err("help requested".to_owned()),
        other => return Err(format!("unknown command {other}")),
    };
    Ok(RuntimeConfig {
        command,
        ffplay_path,
    })
}

fn parse_port(value: String) -> Result<u16, String> {
    value
        .parse::<u16>()
        .map_err(|_| format!("invalid UDP port {value}"))
        .and_then(|port| {
            if port == 0 {
                Err("UDP port must be greater than zero".to_owned())
            } else {
                Ok(port)
            }
        })
}

fn value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{option} requires a value"))
}

fn usage(program: &str) -> String {
    format!(
        "Usage:\n  {program} media --port <udp-port> [--ffplay-path <path>]\n\nEnvironment:\n  FOURPLAY_FFPLAY_PATH    Default ffplay executable path."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_media_command() {
        let config = parse_args([
            "media".to_owned(),
            "--port".to_owned(),
            "41000".to_owned(),
            "--ffplay-path".to_owned(),
            "ffplay-custom".to_owned(),
        ])
        .unwrap();

        assert_eq!(
            config,
            RuntimeConfig {
                command: RuntimeCommand::Media { port: 41_000 },
                ffplay_path: PathBuf::from("ffplay-custom"),
            }
        );
    }

    #[test]
    fn rejects_missing_port() {
        assert!(
            parse_args(["media".to_owned()])
                .unwrap_err()
                .contains("missing --port")
        );
    }

    #[test]
    fn rejects_zero_port() {
        assert!(
            parse_args(["media".to_owned(), "--port".to_owned(), "0".to_owned()])
                .unwrap_err()
                .contains("greater than zero")
        );
    }

    #[test]
    fn usage_mentions_media_command() {
        assert!(usage("seat-client-runtime").contains("media --port"));
    }
}
