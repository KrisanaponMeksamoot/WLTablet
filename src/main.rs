mod signaling;
mod webrtc_core;
mod host_control;

#[cfg(target_os = "linux")]
mod dbus_portal;

use std::{
    error::Error, fmt::{self, Display}, net::SocketAddr, str::FromStr, sync::{
        Arc, atomic::AtomicBool,
    },
};

use clap::Parser;
use rand::RngExt;
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
    pin: Option<Pin>,

    #[arg(short, long, default_value_t = SocketAddr::from(([0, 0, 0, 0], 8080)))]
    listen: SocketAddr
}

#[derive(Clone, Debug)]
pub struct Pin ([u8; 6]);

impl Pin {
    fn as_str(&self) -> &str {
        str::from_utf8(&self.0).unwrap()
    }
}

impl FromStr for Pin {
    type Err = fmt::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() == 6 && s.as_bytes().iter().all(|&x| x.is_ascii_digit()) {
            let b: [u8; 6] = s.as_bytes()[0..6].try_into().unwrap();
            Ok(Pin(b))
        } else {
            Err(fmt::Error{})
        }
    }
}

impl Display for Pin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct AppState {
    pub pin: Arc<Pin>,
    pub pairing_allowed: Arc<AtomicBool>,
    pub webrtc: Arc<WebRtcCore>,
    pub control_wrapper: Arc<dyn ControlWrapper>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let pin: Arc<Pin> = Arc::new(match args.pin {
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

fn generate_pairing_pin() -> std::io::Result<Pin> {
    let mut rng = rand::rng();
    let value = rng.random_range(0..1_000_000);
    Ok(Pin::from_str(&format!("{:06}", value)).unwrap())
}
