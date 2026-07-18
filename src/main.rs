mod dbus_portal;
mod signaling;
mod webrtc_core;
mod host_control;

use std::{
    error::Error, fs::File, io::Read, net::SocketAddr, sync::{
        Arc, atomic::AtomicBool,
    },
};

use clap::Parser;
use regex::Regex;
use tokio::net::TcpListener;

use crate::{dbus_portal::WaylandControlWrapper, host_control::{ControlWrapper, X11ControlWrapper}};
use crate::webrtc_core::WebRtcCore;

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    #[arg(short, long)]
    pin: Option<String>
}

#[derive(Clone)]
pub struct AppState {
    pub pin: Arc<String>,
    pub pairing_allowed: Arc<AtomicBool>,
    pub webrtc: Arc<WebRtcCore>,
    pub control_wrapper: Arc<dyn ControlWrapper>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    if let Some(pin) = &args.pin {
        let re = Regex::new("\\d{6}").unwrap();
        if pin.len() != 6 || !re.is_match(&pin) {
            panic!("Invalid PIN format");
        }
    }

    let pin: Arc<String> = Arc::new(match args.pin {
        None => generate_pairing_pin().unwrap(),
        Some(pin) => pin
    });

    println!();
    println!("\x1b[1;32mWLTablet pairing PIN: {}\x1b[0m", pin);
    println!();

    
    let control_wrapper: Arc<dyn ControlWrapper> = if std::env::var("WAYLAND_DISPLAY").is_ok() {
        println!("Detected Wayland, using RemoteDesktopPortal");
        Arc::new(WaylandControlWrapper::new().await?)
    } else {
        println!("Detected X11, using X11CursorControl");
        Arc::new(X11ControlWrapper::new()?)
    };

    let pairing_allowed = Arc::new(AtomicBool::new(true));
    let webrtc_core = Arc::new(WebRtcCore::new(control_wrapper.clone()).await);

    let state = Arc::new(AppState { pin, pairing_allowed, webrtc: webrtc_core, control_wrapper });
    let app = signaling::build_router(state.clone());

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = TcpListener::bind(addr).await?;
    println!("Listening on http://{}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

fn generate_pairing_pin() -> std::io::Result<String> {
    let mut source = File::open("/dev/urandom")?;
    let mut bytes = [0u8; 4];
    source.read_exact(&mut bytes)?;
    let value = u32::from_ne_bytes(bytes) % 1_000_000;
    Ok(format!("{:06}", value))
}
