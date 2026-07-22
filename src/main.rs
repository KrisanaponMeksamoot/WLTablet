mod signaling;
mod webrtc_core;
mod host_control;

#[cfg(target_os = "linux")]
mod dbus_portal;

use std::{
    error::Error, net::SocketAddr, sync::{
        Arc, atomic::AtomicBool,
    },
};

use clap::Parser;
use rand::RngExt;
use regex::Regex;
use tokio::net::TcpListener;

#[cfg(target_os = "linux")]
use crate::{dbus_portal::WaylandControlWrapper, host_control::{ControlWrapper, X11ControlWrapper}};

#[cfg(target_os = "windows")]
use crate::{host_control::{ControlWrapper, WindowsControlWrapper}};

use crate::webrtc_core::WebRtcCore;

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    #[arg(short, long)]
    pin: Option<String>,

    #[arg(short, long, default_value_t = SocketAddr::from(([0, 0, 0, 0], 8080)))]
    listen: SocketAddr
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
    
    let addr = args.listen;

    println!();
    println!("\x1b[1;32mWLTablet pairing PIN: {}\x1b[0m", pin);
    println!();

    
    #[cfg(target_os = "linux")]
    let control_wrapper: Arc<dyn ControlWrapper> =
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            println!("Detected Wayland, using RemoteDesktopPortal");
            Arc::new(WaylandControlWrapper::new().await?)
        } else {
            println!("Detected X11, using X11CursorControl");
            Arc::new(X11ControlWrapper::new()?)
        };

    #[cfg(target_os = "windows")]
    let control_wrapper: Arc<dyn ControlWrapper> = {
        println!("Detected Windows, using Windows cursor control");
        Arc::new(WindowsControlWrapper::new()?)
    };

    let pairing_allowed = Arc::new(AtomicBool::new(true));
    let webrtc_core = Arc::new(WebRtcCore::new(control_wrapper.clone()).await);

    let state = Arc::new(AppState { pin, pairing_allowed, webrtc: webrtc_core, control_wrapper });
    let app = signaling::build_router(state.clone());

    let listener = TcpListener::bind(addr).await?;
    println!("Listening on http://{}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

fn generate_pairing_pin() -> std::io::Result<String> {
    let mut rng = rand::rng();
    let value = rng.random_range(0..1_000_000);
    Ok(format!("{:06}", value))
}
