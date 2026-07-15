mod dbus_portal;
mod signaling;
mod webrtc_core;
mod host_control;

use std::{
    collections::HashMap, error::Error, fs::File, io::Read, net::SocketAddr, sync::{
        Arc, atomic::AtomicBool,
    },
};

use tokio::net::TcpListener;
use zbus::zvariant::Value;

use crate::{dbus_portal::{RemoteDesktopPortal, WaylandControlWrapper}, host_control::{ControlWrapper, CursorControl, ScreenInfo, X11ControlWrapper, X11CursorControl}};
use crate::webrtc_core::WebRtcCore;

#[derive(Clone)]
pub struct AppState {
    pub pin: Arc<String>,
    pub pairing_allowed: Arc<AtomicBool>,
    pub webrtc: Arc<WebRtcCore>,
    pub control_wrapper: Arc<dyn ControlWrapper>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let pin: Arc<String> = Arc::new(generate_pairing_pin()?);
    // let pin: Arc<String> = Arc::new("676767".to_string()); // testing

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
    let webrtc_core = Arc::new(WebRtcCore::new(control_wrapper.clone(), pairing_allowed.clone()).await);

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
