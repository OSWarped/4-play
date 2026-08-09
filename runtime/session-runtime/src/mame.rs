use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};

#[derive(Debug, Clone)]
pub struct MameConfig {
    pub binary: PathBuf,
    pub ini_path: PathBuf,
    pub controller_path: PathBuf,
    pub controller_profile: String,
    pub rom: String,
    pub working_directory: PathBuf,
    pub video_path: PathBuf,
    pub audio_path: PathBuf,
}

pub struct MameProcess {
    child: Child,
}

impl MameProcess {
    pub fn spawn(config: &MameConfig) -> io::Result<Self> {
        let cfg_directory = config.working_directory.join("cfg");
        let nvram_directory = config.working_directory.join("nvram");
        let state_directory = config.working_directory.join("state");
        let snapshot_directory = config.working_directory.join("snap");
        let diff_directory = config.working_directory.join("diff");

        validate_path(&config.binary, "MAME binary")?;
        validate_path(&config.ini_path, "MAME INI path")?;
        validate_path(&config.controller_path, "MAME controller path")?;

        let controller_file = config
            .controller_path
            .join(format!("{}.cfg", config.controller_profile));

        validate_path(&controller_file, "MAME controller profile")?;

        let mut command = Command::new(&config.binary);

        command
            .env("SDL_VIDEODRIVER", "offscreen")
            .env("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1")
            .arg("-inipath")
            .arg(&config.ini_path)
            .arg("-ctrlrpath")
            .arg(&config.controller_path)
            .arg("-ctrlr")
            .arg(&config.controller_profile)
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
            .arg("-background_input")
            .arg("-verbose")
            .arg("-skip_gameinfo")
            .arg("-rawvideowrite")
            .arg(&config.video_path)
            .arg("-rawaudiowrite")
            .arg(&config.audio_path)
            .arg(&config.rom)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let child = command.spawn()?;

        println!(
            "MAME started: PID={} ROM={} controller={}",
            child.id(),
            config.rom,
            config.controller_profile
        );

        Ok(Self { child })
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait()
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    pub fn terminate(&mut self) -> io::Result<()> {
        if self.child.try_wait()?.is_none() {
            self.child.kill()?;
        }

        Ok(())
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
