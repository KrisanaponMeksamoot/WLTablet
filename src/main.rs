mod dbus_portal;
mod signaling;
mod webrtc_core;

use std::{
    collections::HashMap, error::Error, fs::File, io::Read, net::SocketAddr, sync::{
        Arc, atomic::AtomicBool,
    },
};

use tokio::net::TcpListener;
use zbus::zvariant::Value;

use crate::dbus_portal::{RemoteDesktopPortal, X11CursorControl, CursorControl};
use crate::webrtc_core::WebRtcCore;

#[derive(Clone)]
pub struct AppState {
    pub pin: Arc<String>,
    pub pairing_allowed: Arc<AtomicBool>,
    pub webrtc: Arc<WebRtcCore>,
    pub cursor_control: Arc<dyn CursorControl>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let pin: Arc<String> = Arc::new(generate_pairing_pin()?);
    // let pin: Arc<String> = Arc::new("676767".to_string()); // testing

    println!();
    println!("\x1b[1;32mWLTablet pairing PIN: {}\x1b[0m", pin);
    println!();

    let session_handle_token = RemoteDesktopPortal::generate_handle_token();
    
    let cursor_control: Arc<dyn CursorControl> = if std::env::var("WAYLAND_DISPLAY").is_ok() {
        println!("Detected Wayland, using RemoteDesktopPortal");
        let mut portal = RemoteDesktopPortal::new().await?;
        portal.create_session(vec![("session_handle_token", Value::from(session_handle_token))].into_iter().collect()).await?;
        portal.select_devices(HashMap::new()).await?;
        portal.start("", HashMap::new()).await?;
        Arc::new(portal)
    } else {
        println!("Detected X11, using X11CursorControl");
        Arc::new(X11CursorControl::new()?)
    };

    let pairing_allowed = Arc::new(AtomicBool::new(true));
    let webrtc_core = Arc::new(WebRtcCore::new(cursor_control.clone(), pairing_allowed.clone()).await);

    let state = Arc::new(AppState { pin, pairing_allowed, webrtc: webrtc_core, cursor_control });
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
