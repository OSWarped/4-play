mod media_bridge;
mod session;

use media_bridge::{MediaBridge, MediaBridgeConfig};
use session::{Session, SessionConfig};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = SessionConfig {
        id: 2,
        rom: "aliens".to_string(),
        width: 288,
        height: 224,
        fps: 59.1856,
        destination_ip: "192.168.20.10".to_string(),
        udp_port: 5001,
    };

    let mut session = Session::new(config);
    session.prepare()?;
    session.create_media_endpoints()?;

    println!("{session}");

    let mut bridge = MediaBridge::new(MediaBridgeConfig {
        video_path: session.video_path(),
        audio_path: session.audio_path(),
        width: session.config.width,
        height: session.config.height,
    });

    bridge.start();

    println!("Media bridge started. Launch MAME in another terminal.");

    bridge.monitor_for(Duration::from_secs(30));
    bridge.stop()?;

    Ok(())
}
