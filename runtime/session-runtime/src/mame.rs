use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};

const CONTROLLER_PROFILE_NAME: &str = "4play-session";

#[derive(Debug, Clone)]
pub struct MameConfig {
    pub binary: PathBuf,
    pub ini_path: PathBuf,
    pub rom: String,
    pub working_directory: PathBuf,
    pub video_path: PathBuf,
    pub audio_path: PathBuf,
    pub controller_device_id: Option<String>,
}

pub struct MameProcess {
    child: Option<Child>,
}

impl MameProcess {
    pub fn spawn(config: &MameConfig) -> io::Result<Self> {
        let cfg_directory = config.working_directory.join("cfg");
        let nvram_directory = config.working_directory.join("nvram");
        let state_directory = config.working_directory.join("state");
        let snapshot_directory = config.working_directory.join("snap");
        let diff_directory = config.working_directory.join("diff");
        let controller_directory = config.working_directory.join("ctrlr");

        validate_path(&config.binary, "MAME binary")?;
        validate_path(&config.ini_path, "MAME INI path")?;

        let mut command = Command::new(&config.binary);

        command
            .env("SDL_VIDEODRIVER", "offscreen")
            .arg("-inipath")
            .arg(&config.ini_path)
            .arg("-cfg_directory")
            .arg(cfg_directory)
            .arg("-nvram_directory")
            .arg(nvram_directory)
            .arg("-state_directory")
            .arg(state_directory)
            .arg("-snapshot_directory")
            .arg(snapshot_directory)
            .arg("-diff_directory")
            .arg(diff_directory)
            .arg("-sound")
            .arg("none")
            .arg("-skip_gameinfo")
            .arg("-rawvideowrite")
            .arg(&config.video_path)
            .arg("-rawaudiowrite")
            .arg(&config.audio_path);

        if let Some(device_id) = config.controller_device_id.as_deref() {
            fs::create_dir_all(&controller_directory)?;
            fs::write(
                controller_directory.join(format!("{CONTROLLER_PROFILE_NAME}.cfg")),
                controller_profile(device_id),
            )?;
            command
                .arg("-ctrlrpath")
                .arg(&controller_directory)
                .arg("-ctrlr")
                .arg(CONTROLLER_PROFILE_NAME);
        }

        command
            .arg(&config.rom)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let child = command.spawn()?;

        println!("MAME started: PID={} ROM={}", child.id(), config.rom);

        Ok(Self { child: Some(child) })
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        match self.child.as_mut() {
            Some(child) => child.try_wait(),
            None => Ok(None),
        }
    }

    pub fn terminate(&mut self) -> io::Result<Option<ExitStatus>> {
        let Some(mut child) = self.child.take() else {
            return Ok(None);
        };

        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }

        child.kill()?;
        child.wait().map(Some)
    }
}

fn controller_profile(device_id: &str) -> String {
    format!(
        r#"<?xml version="1.0"?>
<mameconfig version="10">
    <system name="default">
        <input>
            <mapdevice device="{device_id}" controller="JOYCODE_1" />
            <port type="P1_JOYSTICK_UP"><newseq type="standard">JOYCODE_1_YAXIS_UP_SWITCH</newseq></port>
            <port type="P1_JOYSTICK_DOWN"><newseq type="standard">JOYCODE_1_YAXIS_DOWN_SWITCH</newseq></port>
            <port type="P1_JOYSTICK_LEFT"><newseq type="standard">JOYCODE_1_XAXIS_LEFT_SWITCH</newseq></port>
            <port type="P1_JOYSTICK_RIGHT"><newseq type="standard">JOYCODE_1_XAXIS_RIGHT_SWITCH</newseq></port>
            <port type="P1_BUTTON1"><newseq type="standard">JOYCODE_1_BUTTON1</newseq></port>
            <port type="P1_BUTTON2"><newseq type="standard">JOYCODE_1_BUTTON2</newseq></port>
            <port type="P1_BUTTON3"><newseq type="standard">JOYCODE_1_BUTTON3</newseq></port>
            <port type="P1_BUTTON4"><newseq type="standard">JOYCODE_1_BUTTON4</newseq></port>
            <port type="P1_BUTTON5"><newseq type="standard">JOYCODE_1_BUTTON5</newseq></port>
            <port type="P1_BUTTON6"><newseq type="standard">JOYCODE_1_BUTTON6</newseq></port>
            <port type="COIN1"><newseq type="standard">JOYCODE_1_BUTTON7</newseq></port>
            <port type="START1"><newseq type="standard">JOYCODE_1_BUTTON8</newseq></port>
        </input>
    </system>
</mameconfig>
"#
    )
}

impl Drop for MameProcess {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };

        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
    }
}

fn validate_path(path: &Path, label: &str) -> io::Result<()> {
    if path.exists() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{label} does not exist: {}", path.display()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::controller_profile;

    #[test]
    fn controller_profile_maps_only_the_assigned_device_to_player_one() {
        let profile = controller_profile("011200007856000034120000");

        assert!(profile.contains(
            "<mapdevice device=\"011200007856000034120000\" controller=\"JOYCODE_1\" />"
        ));
        assert!(profile.contains("JOYCODE_1_BUTTON6"));
        assert!(profile.contains("JOYCODE_1_BUTTON8"));
        assert!(!profile.contains("JOYCODE_2"));
    }
}
