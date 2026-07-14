use std::error::Error;
use std::sync::Arc;
use std::pin::Pin;
use std::future::Future;
use std::{
    collections::HashMap,
    fmt::Write,
    fs::File,
    io::Read,
};

use futures_util::StreamExt;
use tokio::sync::Mutex;
use x11rb::protocol::xproto::{Screen, warp_pointer};
use x11rb::protocol::xtest::fake_input;
use zbus::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub trait CursorControl: Send + Sync {
    fn move_cursor(&self, x: u32, y: u32) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
    fn press_button(&self, button: u8) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
    fn release_button(&self, button: u8) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
    fn get_screen_dimensions(&self) -> (u32, u32);
}

#[derive(Debug)]
pub struct RemoteDesktopPortalProxy {
    pub conn: Connection,
    pub proxy: Proxy<'static>,
    operation_lock: Mutex<()>,
}

impl RemoteDesktopPortalProxy {
    pub async fn new() -> Result<Self> {
        let conn = Connection::session().await?;
        let proxy = Proxy::new(
            &conn,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.RemoteDesktop",
        )
        .await?;
        Ok(Self { conn, proxy, operation_lock: Mutex::new(()) })
    }

    pub async fn create_session(&self, options: HashMap<&str, Value<'_>>) -> Result<OwnedObjectPath> {
        Ok(self.proxy.call("CreateSession", &(options)).await?)
    }

    pub async fn select_devices(&self, session_handle: &OwnedObjectPath, options: HashMap<&str, Value<'_>>) -> Result<()> {
        Ok(self.proxy.call("SelectDevices", &(session_handle, options)).await?)
    }

    pub async fn start(&self, session_handle: &OwnedObjectPath, parent_window: &str, options: HashMap<&str, Value<'_>>) -> Result<()> {
        Ok(self.proxy.call("Start", &(session_handle, parent_window, options)).await?)
    }

    pub async fn notify_pointer_absolute(&self, session_handle: &OwnedObjectPath, options: HashMap<&str, Value<'_>>, x: u32, y: u32) -> Result<()> {
        let _guard = self.operation_lock.lock().await;
        Ok(self.proxy.call("NotifyPointerAbsolute", &(session_handle, options, x, y)).await?)
    }
}

#[derive(Debug)]
pub struct RemoteDesktopPortal {
    inner: RemoteDesktopPortalProxy,
    session_handle: Option<OwnedObjectPath>,
}

impl RemoteDesktopPortal {
    pub async fn new() -> Result<Self> {
        let inner = RemoteDesktopPortalProxy::new().await?;
        Ok(Self { inner, session_handle: None })
    }

    pub async fn create_session(&mut self, options: HashMap<&str, Value<'_>>) -> Result<()> {
        let handle = self.inner.create_session(options).await?;
        let result = wait_for_portal_response(&self.inner.conn, handle).await?;
        let session_handle_str = result.get("session_handle")
            .and_then(|v| Some(v.downcast_ref::<String>())).ok_or(Arc::new(std::io::Error::new(std::io::ErrorKind::Other, "Missing session_handle in response")))??;
        self.session_handle = Some(OwnedObjectPath::try_from(session_handle_str.as_str())?);
        Ok(())
    }

    pub async fn select_devices(&self, options: HashMap<&str, Value<'_>>) -> Result<()> {
        let session_handle = self.session_handle.as_ref()
            .ok_or(Arc::new(std::io::Error::new(std::io::ErrorKind::Other, "No session created")))?;
        Ok(self.inner.select_devices(session_handle, options).await?)
    }

    pub async fn start(&self, parent_window: &str, options: HashMap<&str, Value<'_>>) -> Result<()> {
        let session_handle = self.session_handle.as_ref()
            .ok_or(Arc::new(std::io::Error::new(std::io::ErrorKind::Other, "No session created")))?;
        Ok(self.inner.start(session_handle, parent_window, options).await?)
    }

    pub async fn notify_pointer_absolute(&self, x: u32, y: u32, options: HashMap<&str, Value<'_>>) -> Result<()> {
        let session_handle = self.session_handle.as_ref()
            .ok_or(Arc::new(std::io::Error::new(std::io::ErrorKind::Other, "No session created")))?;
        Ok(self.inner.notify_pointer_absolute(session_handle, options, x, y).await?)
    }

    pub fn session_handle(&self) -> Option<&OwnedObjectPath> {
        self.session_handle.as_ref()
    }

    pub fn generate_handle_token() -> String {
        let mut buffer = [0u8; 16];
        if let Ok(mut file) = File::open("/dev/urandom") {
            let _ = file.read_exact(&mut buffer);
        } else {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u128;
            buffer.copy_from_slice(&nanos.to_le_bytes());
        }
        let mut token = String::with_capacity(32);
        for byte in buffer.iter() {
            write!(token, "{:02x}", byte).unwrap();
        }
        token
    }
}

impl CursorControl for RemoteDesktopPortal {
    fn move_cursor(&self, x: u32, y: u32) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(self.notify_pointer_absolute(x, y, HashMap::new()))
    }

    fn press_button(&self, _button: u8) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }

    fn release_button(&self, _button: u8) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }

    fn get_screen_dimensions(&self) -> (u32, u32) {
        get_wayland_screen_dimensions().unwrap_or((1920, 1080))
    }
}

#[derive(Debug)]
pub struct X11CursorControl {
    connection: x11rb::rust_connection::RustConnection,
    screen_num: usize,
    screen: Screen,
}

impl X11CursorControl {
    pub fn new() -> Result<Self> {
        let (connection, screen_num) = x11rb::connect(None)?;
        let screen = x11rb::connection::Connection::setup(&connection).roots[screen_num].clone();
        Ok(Self { connection, screen_num, screen })
    }
}

impl CursorControl for X11CursorControl {
    fn move_cursor(&self, x: u32, y: u32) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            warp_pointer(
                &self.connection,
                x11rb::NONE,
                self.screen.root,
                0,
                0,
                0,
                0,
                x as i16,
                y as i16
            )?;
            x11rb::connection::Connection::flush(&self.connection)?;
            Ok(())
        })
    }

    fn press_button(&self, button: u8) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            fake_input(&self.connection, 4, button, 0, 0, 0, 0, 0)
                .map_err(|e| Arc::new(std::io::Error::new(std::io::ErrorKind::Other, format!("X11 button press failed: {}", e))))?;
            x11rb::connection::Connection::flush(&self.connection)?;
            Ok(())
        })
    }

    fn release_button(&self, button: u8) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            fake_input(&self.connection, 5, button, 0, 0, 0, 0, 0)
                .map_err(|e| Arc::new(std::io::Error::new(std::io::ErrorKind::Other, format!("X11 button release failed: {}", e))))?;
            x11rb::connection::Connection::flush(&self.connection)?;
            Ok(())
        })
    }

    fn get_screen_dimensions(&self) -> (u32, u32) {
        let screen = &self.screen;
        (screen.width_in_pixels as u32, screen.height_in_pixels as u32)
    }
}

pub async fn wait_for_portal_response(
    conn: &Connection,
    request_path: OwnedObjectPath,
) -> Result<HashMap<String, OwnedValue>> {
    let proxy = Proxy::new(
        conn, 
        "org.freedesktop.portal.Desktop",
        request_path,
        "org.freedesktop.portal.Request"
    ).await?;

    let mut signal_stream = proxy.receive_signal("Response").await?;

    println!("Waiting for desktop portal interaction...");

    if let Some(msg) = signal_stream.next().await {
        let body = msg.body();
        let (status_code, results): (u32, HashMap<String, OwnedValue>) = body.deserialize().map_err(|e| zbus::Error::from(e))?;

        match status_code {
            0 => Ok(results),
            1 => Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "User cancelled the portal request"))),
            _ => Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, format!("Portal request failed with status code: {}", status_code)))),
        }
    } else {
        Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Signal stream terminated unexpectedly")))
    }
}

fn get_wayland_screen_dimensions() -> Option<(u32, u32)> {
    if let Ok(output) = std::process::Command::new("wc")
        .arg("-c")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        if output.status.success() {
            return Some((1920, 1080));
        }
    }
    None
}