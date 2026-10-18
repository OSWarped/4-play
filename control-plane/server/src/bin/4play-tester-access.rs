//! Local operator CLI. There is deliberately no public HTTP management endpoint.
use std::{
    error::Error,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use control_plane_server::tester_access::TesterAccess;
use uuid::Uuid;

const USAGE: &str = "Usage (on Linux as the service user):
  4play-tester-access issue --database <private-file> [--hours <1-168>] [--seats <1-8>]
  4play-tester-access list --database <private-file>
  4play-tester-access revoke --database <private-file> --id <invitation-uuid>

The parent directory must already exist and be private. Issue prints a secret
invitation code ONCE; never paste it into logs, source control, or public URLs.
These invitations do not enable public access until the WAN gate is integrated.";

struct Options {
    command: String,
    database: PathBuf,
    hours: u64,
    seats: u32,
    id: Option<Uuid>,
}
fn parse(args: &[String]) -> Result<Options, &'static str> {
    let command = args.first().ok_or(USAGE)?.as_str();
    if !["issue", "list", "revoke"].contains(&command) {
        return Err(USAGE);
    }
    let mut database = None;
    let mut hours = None;
    let mut seats = None;
    let mut id = None;
    let mut pairs = args[1..].chunks_exact(2);
    for pair in &mut pairs {
        match pair[0].as_str() {
            "--database" if database.is_none() && !pair[1].is_empty() => {
                database = Some(PathBuf::from(&pair[1]))
            }
            "--hours" if command == "issue" && hours.is_none() => {
                hours = Some(pair[1].parse::<u64>().map_err(|_| USAGE)?)
            }
            "--seats" if command == "issue" && seats.is_none() => {
                seats = Some(pair[1].parse::<u32>().map_err(|_| USAGE)?)
            }
            "--id" if command == "revoke" && id.is_none() => {
                id = Some(Uuid::parse_str(&pair[1]).map_err(|_| USAGE)?)
            }
            _ => return Err(USAGE),
        }
    }
    if !pairs.remainder().is_empty() || (command == "revoke" && id.is_none()) {
        return Err(USAGE);
    }
    let hours = hours.unwrap_or(24);
    let seats = seats.unwrap_or(1);
    if !(1..=168).contains(&hours) || !(1..=8).contains(&seats) {
        return Err(USAGE);
    }
    Ok(Options {
        command: command.into(),
        database: database.ok_or(USAGE)?,
        hours,
        seats,
        id,
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 1 && ["--help", "-h"].contains(&args[0].as_str()) {
        println!("{USAGE}");
        return Ok(());
    }
    let options = parse(&args)?;
    if !cfg!(unix) {
        return Err(
            "Manage tester invitations on the Linux server; Unix private-file checks are required"
                .into(),
        );
    }
    // Avoid creating an empty database for a mistyped list/revoke path.
    if options.command != "issue" && !options.database.is_file() {
        return Err("Private tester database does not exist".into());
    }
    let access = TesterAccess::open(&options.database).await?;
    match options.command.as_str() {
        "issue" => {
            let now = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
            let invite = access
                .issue(
                    now,
                    now.checked_add(options.hours * 3_600_000)
                        .ok_or("Invalid expiration")?,
                    options.seats,
                )
                .await?;
            println!(
                "Invitation ID: {}\nExpires in {} hours; browser enrollments: {}\nInvitation code (shown once): {}",
                invite.id, options.hours, options.seats, invite.code
            );
        }
        "list" => println!("{}", serde_json::to_string_pretty(&access.list().await?)?),
        "revoke" => {
            if access.revoke(options.id.ok_or(USAGE)?).await? {
                println!("Invitation revoked; its admissions are no longer authorized.");
            } else {
                println!("Invitation not found or already revoked.");
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options(args: &[&str]) -> Result<Options, &'static str> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }
    #[test]
    fn valid_commands_and_private_defaults() {
        let issue = options(&["issue", "--database", "private/invites.db"]).unwrap();
        assert_eq!(issue.hours, 24);
        assert_eq!(issue.seats, 1);
        assert!(options(&["list", "--database", "private/invites.db"]).is_ok());
        assert!(
            options(&[
                "revoke",
                "--database",
                "db",
                "--id",
                "c6911abf-6da8-4d4d-8089-62265db70328"
            ])
            .is_ok()
        );
    }
    #[test]
    fn unknown_duplicate_and_out_of_range_options_are_rejected() {
        for args in [
            vec![],
            vec!["issue"],
            vec!["revoke", "--database", "db"],
            vec!["list", "--database", "db", "--hours", "1"],
            vec!["issue", "--database", "db", "--database", "other"],
            vec!["issue", "--database", "db", "--hours", "169"],
            vec!["issue", "--database", "db", "--hours", "0"],
            vec!["issue", "--database", "db", "--seats", "9"],
            vec!["issue", "--database", "db", "--seats", "0"],
            vec!["issue", "--database", "db", "--hours"],
            vec!["issue", "--database", "db", "--id", "unknown"],
            vec!["revoke", "--database", "db", "--id", "invalid"],
        ] {
            assert!(options(&args).is_err(), "{args:?}");
        }
    }
}
