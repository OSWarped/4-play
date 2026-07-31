mod session;

use session::{Session, SessionConfig};

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

    Ok(())
}
