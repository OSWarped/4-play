use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const CONTROLLER_PROFILE_NAME: &str = "4play-session";
const CONTROL_SCRIPT_NAME: &str = "4play-runtime-control.lua";
const STOP_REQUEST_NAME: &str = "stop-mame.request";
const GRACEFUL_STOP_TIMEOUT: Duration = Duration::from_secs(3);
const GRACEFUL_STOP_POLL: Duration = Duration::from_millis(20);

#[derive(Debug, Clone)]
pub struct MameConfig {
    pub binary: PathBuf,
    pub ini_path: PathBuf,
    pub rom: String,
    pub working_directory: PathBuf,
    pub video_path: PathBuf,
    pub audio_path: PathBuf,
    pub controller_device_ids: Vec<(u8, String)>,
    pub autosave: bool,
}

pub struct MameProcess {
    child: Option<Child>,
    stop_request_path: PathBuf,
}

impl MameProcess {
    pub fn spawn(config: &MameConfig) -> io::Result<Self> {
        let cfg_directory = config.working_directory.join("cfg");
        let nvram_directory = config.working_directory.join("nvram");
        let state_directory = config.working_directory.join("state");
        let snapshot_directory = config.working_directory.join("snap");
        let diff_directory = config.working_directory.join("diff");
        let controller_directory = config.working_directory.join("ctrlr");
        let control_script_path = config.working_directory.join(CONTROL_SCRIPT_NAME);
        let stop_request_path = config.working_directory.join(STOP_REQUEST_NAME);

        validate_path(&config.binary, "MAME binary")?;
        validate_path(&config.ini_path, "MAME INI path")?;
        remove_if_present(&stop_request_path)?;
        fs::write(
            &control_script_path,
            runtime_control_script(&stop_request_path),
        )?;

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
            .arg("-joystick")
            .arg("-joystickprovider")
            .arg("sdljoy")
            .arg("-autoboot_script")
            .arg(&control_script_path)
            .arg("-autoboot_delay")
            .arg("0")
            .arg("-skip_gameinfo")
            .arg("-rawvideowrite")
            .arg(&config.video_path)
            .arg("-rawaudiowrite")
            .arg(&config.audio_path);

        if config.autosave {
            command.arg("-autosave");
        }

        if !config.controller_device_ids.is_empty() {
            fs::create_dir_all(&controller_directory)?;
            fs::write(
                controller_directory.join(format!("{CONTROLLER_PROFILE_NAME}.cfg")),
                controller_profile(&config.controller_device_ids),
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

        Ok(Self {
            child: Some(child),
            stop_request_path,
        })
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
            remove_if_present(&self.stop_request_path)?;
            return Ok(Some(status));
        }

        if let Err(error) = fs::write(&self.stop_request_path, b"stop\n") {
            eprintln!(
                "Failed to request graceful MAME shutdown through {}: {error}",
                self.stop_request_path.display()
            );
        } else {
            let started = Instant::now();
            while started.elapsed() < GRACEFUL_STOP_TIMEOUT {
                if let Some(status) = child.try_wait()? {
                    remove_if_present(&self.stop_request_path)?;
                    return Ok(Some(status));
                }
                thread::sleep(GRACEFUL_STOP_POLL);
            }
            eprintln!(
                "MAME did not exit within {} ms; forcing termination.",
                GRACEFUL_STOP_TIMEOUT.as_millis()
            );
        }

        child.kill()?;
        let status = child.wait().map(Some);
        remove_if_present(&self.stop_request_path)?;
        status
    }
}

fn runtime_control_script(stop_request_path: &Path) -> String {
    format!(
        r#"local stop_request_path = [[{}]]

emu.register_periodic(function()
    local request = io.open(stop_request_path, "r")
    if request then
        request:close()
        os.remove(stop_request_path)
        manager.machine:exit()
    end
end)
"#,
        stop_request_path.display()
    )
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn controller_profile(device_ids: &[(u8, String)]) -> String {
    let mut input = String::new();
    for (player_number, device_id) in device_ids {
        let joycode = format!("JOYCODE_{player_number}");
        input.push_str(&format!(
            r#"            <mapdevice device="{device_id}" controller="{joycode}" />
            <port type="P{player_number}_JOYSTICK_UP"><newseq type="standard">{joycode}_YAXIS_UP_SWITCH</newseq></port>
            <port type="P{player_number}_JOYSTICK_DOWN"><newseq type="standard">{joycode}_YAXIS_DOWN_SWITCH</newseq></port>
            <port type="P{player_number}_JOYSTICK_LEFT"><newseq type="standard">{joycode}_XAXIS_LEFT_SWITCH</newseq></port>
            <port type="P{player_number}_JOYSTICK_RIGHT"><newseq type="standard">{joycode}_XAXIS_RIGHT_SWITCH</newseq></port>
            <port type="P{player_number}_BUTTON1"><newseq type="standard">{joycode}_BUTTON1</newseq></port>
            <port type="P{player_number}_BUTTON2"><newseq type="standard">{joycode}_BUTTON2</newseq></port>
            <port type="P{player_number}_BUTTON3"><newseq type="standard">{joycode}_BUTTON3</newseq></port>
            <port type="P{player_number}_BUTTON4"><newseq type="standard">{joycode}_BUTTON4</newseq></port>
            <port type="P{player_number}_BUTTON5"><newseq type="standard">{joycode}_BUTTON5</newseq></port>
            <port type="P{player_number}_BUTTON6"><newseq type="standard">{joycode}_BUTTON6</newseq></port>
            <port type="COIN{player_number}"><newseq type="standard">{joycode}_BUTTON7</newseq></port>
            <port type="START{player_number}"><newseq type="standard">{joycode}_BUTTON8</newseq></port>
"#
        ));
    }

    format!(
        r#"<?xml version="1.0"?>
<mameconfig version="10">
    <system name="default">
        <input>
{input}
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
        let _ = remove_if_present(&self.stop_request_path);
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
    use super::{controller_profile, runtime_control_script};
    use std::path::Path;

    #[test]
    fn controller_profile_maps_assigned_devices_to_players() {
        let profile = controller_profile(&[
            (1, "011200007856000034120000".to_owned()),
            (2, "01120000abcd0000ef120000".to_owned()),
        ]);

        assert!(profile.contains(
            "<mapdevice device=\"011200007856000034120000\" controller=\"JOYCODE_1\" />"
        ));
        assert!(profile.contains(
            "<mapdevice device=\"01120000abcd0000ef120000\" controller=\"JOYCODE_2\" />"
        ));
        assert!(profile.contains("JOYCODE_1_BUTTON6"));
        assert!(profile.contains("JOYCODE_2_BUTTON6"));
        assert!(profile.contains("P2_JOYSTICK_UP"));
        assert!(profile.contains("START2"));
    }

    #[test]
    fn runtime_control_script_requests_a_scheduled_machine_exit() {
        let script = runtime_control_script(Path::new("/tmp/session-7/stop-mame.request"));

        assert!(script.contains("/tmp/session-7/stop-mame.request"));
        assert!(script.contains("emu.register_periodic"));
        assert!(script.contains("manager.machine:exit()"));
    }
}
