use std::{fs, path::Path, process::Command};

use control_protocol::{DiscoveredGame, GameRuntimeProfile, RuntimeHostCatalog};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct CatalogManifest {
    schema_version: u32,
    games: Vec<CatalogEntry>,
}

#[derive(Debug, Deserialize)]
struct CatalogEntry {
    id: String,
    rom_name: String,
}

#[derive(Debug, Deserialize)]
struct MameDocument {
    #[serde(rename = "machine", default)]
    machines: Vec<MameMachine>,
}

#[derive(Debug, Deserialize)]
struct MameMachine {
    #[serde(rename = "@name")]
    name: String,
    description: Option<String>,
    #[serde(rename = "display", default)]
    displays: Vec<MameDisplay>,
    input: Option<MameInput>,
    driver: Option<MameDriver>,
}

#[derive(Debug, Deserialize)]
struct MameDisplay {
    #[serde(rename = "@width")]
    width: Option<u32>,
    #[serde(rename = "@height")]
    height: Option<u32>,
    #[serde(rename = "@refresh")]
    refresh_hz: Option<f64>,
    #[serde(rename = "@rotate", default)]
    rotation_degrees: u16,
}

#[derive(Debug, Deserialize)]
struct MameInput {
    #[serde(rename = "@players", default = "one_player")]
    players: u32,
    #[serde(rename = "control", default)]
    controls: Vec<MameControl>,
}

#[derive(Debug, Deserialize)]
struct MameControl {
    #[serde(rename = "@buttons", default)]
    buttons: u32,
}

#[derive(Debug, Deserialize)]
struct MameDriver {
    #[serde(rename = "@savestate")]
    save_state: Option<String>,
}

const fn one_player() -> u32 {
    1
}

pub fn discover_catalog(
    manifest_path: impl AsRef<Path>,
    mame_path: &str,
    mame_ini_path: Option<&str>,
) -> Result<RuntimeHostCatalog, String> {
    let manifest_text = fs::read_to_string(manifest_path.as_ref()).map_err(|error| {
        format!(
            "failed to read catalog manifest {}: {error}",
            manifest_path.as_ref().display()
        )
    })?;
    let manifest = serde_json::from_str::<CatalogManifest>(&manifest_text)
        .map_err(|error| format!("invalid catalog manifest: {error}"))?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "unsupported catalog schema version {}",
            manifest.schema_version
        ));
    }

    let mut games = Vec::with_capacity(manifest.games.len());
    for entry in manifest.games {
        validate_catalog_entry(&entry)?;
        let mut verification = Command::new(mame_path);
        if let Some(path) = mame_ini_path {
            verification.args(["-inipath", path]);
        }
        let verification = verification
            .args(["-verifyroms", &entry.rom_name])
            .output()
            .map_err(|error| format!("failed to verify {}: {error}", entry.rom_name))?;
        if !verification.status.success() {
            return Err(format!(
                "catalog ROM {} is not installed or failed verification",
                entry.rom_name
            ));
        }

        let mut metadata = Command::new(mame_path);
        if let Some(path) = mame_ini_path {
            metadata.args(["-inipath", path]);
        }
        let output = metadata
            .args(["-listxml", &entry.rom_name])
            .output()
            .map_err(|error| format!("failed to run MAME for {}: {error}", entry.rom_name))?;
        if !output.status.success() {
            return Err(format!(
                "MAME metadata query failed for {} with status {}",
                entry.rom_name, output.status
            ));
        }
        let xml = String::from_utf8(output.stdout)
            .map_err(|error| format!("MAME XML for {} is not UTF-8: {error}", entry.rom_name))?;
        games.push(parse_mame_game(&entry, &xml)?);
    }
    games.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(RuntimeHostCatalog { games })
}

fn validate_catalog_entry(entry: &CatalogEntry) -> Result<(), String> {
    for (field, value) in [("game ID", &entry.id), ("ROM name", &entry.rom_name)] {
        let valid = !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
        if !valid {
            return Err(format!(
                "{field} must contain 1-64 ASCII letters, digits, dots, dashes, or underscores"
            ));
        }
    }
    Ok(())
}

fn parse_mame_game(entry: &CatalogEntry, xml: &str) -> Result<DiscoveredGame, String> {
    let document = quick_xml::de::from_str::<MameDocument>(xml)
        .map_err(|error| format!("invalid MAME XML for {}: {error}", entry.rom_name))?;
    let machine = document
        .machines
        .into_iter()
        .find(|machine| machine.name == entry.rom_name)
        .ok_or_else(|| format!("MAME XML did not contain machine {}", entry.rom_name))?;
    let display = machine
        .displays
        .first()
        .ok_or_else(|| format!("{} has no display metadata", entry.rom_name))?;
    let width = display
        .width
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("{} has no valid display width", entry.rom_name))?;
    let height = display
        .height
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("{} has no valid display height", entry.rom_name))?;
    let refresh_hz = display
        .refresh_hz
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| format!("{} has no valid refresh rate", entry.rom_name))?;
    let input = machine.input.as_ref();
    let max_players = input.map_or(1, |input| input.players.max(1));
    let buttons_per_player = input
        .map(|input| {
            input
                .controls
                .iter()
                .map(|control| control.buttons)
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);

    Ok(DiscoveredGame {
        id: entry.id.clone(),
        display_name: machine
            .description
            .unwrap_or_else(|| entry.rom_name.clone()),
        rom_name: entry.rom_name.clone(),
        profile: GameRuntimeProfile {
            width,
            height,
            refresh_hz,
            rotation_degrees: display.rotation_degrees,
            max_players,
            buttons_per_player,
            supports_save_state: machine
                .driver
                .and_then(|driver| driver.save_state)
                .is_some_and(|value| value == "supported"),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::{CatalogEntry, parse_mame_game};

    #[test]
    fn parses_mame_runtime_profile() {
        let xml = r#"<mame>
            <machine name="tmnt">
                <description>Teenage Mutant Ninja Turtles</description>
                <display rotate="0" width="320" height="224" refresh="60.000000" />
                <input players="4" coins="4">
                    <control type="joy" player="1" buttons="2" ways="8" />
                    <control type="joy" player="2" buttons="2" ways="8" />
                </input>
                <driver status="good" savestate="supported" />
            </machine>
        </mame>"#;
        let game = parse_mame_game(
            &CatalogEntry {
                id: "tmnt".to_owned(),
                rom_name: "tmnt".to_owned(),
            },
            xml,
        )
        .unwrap();

        assert_eq!(game.display_name, "Teenage Mutant Ninja Turtles");
        assert_eq!(game.profile.width, 320);
        assert_eq!(game.profile.height, 224);
        assert_eq!(game.profile.refresh_hz, 60.0);
        assert_eq!(game.profile.max_players, 4);
        assert_eq!(game.profile.buttons_per_player, 2);
        assert!(game.profile.supports_save_state);
    }
}
