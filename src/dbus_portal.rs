use std::error::Error;
use std::os::fd::{OwnedFd};
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
use zbus::proxy::SignalStream;
use zbus::{Connection, Proxy, zvariant};
use zbus::zvariant::{Array, Dict, OwnedObjectPath, OwnedValue, Structure, Value};

use crate::host_control::{ControlWrapper, CursorControl, ScreenInfo};

type Result<T> = std::result::Result<T, Box<dyn Error>>;


#[derive(Debug)]
pub struct RemoteDesktopPortalProxy {
    pub conn: Connection,
    pub proxy: Proxy<'static>,
    operation_lock: Mutex<()>,
}

impl RemoteDesktopPortalProxy {
    pub async fn new(conn: &Connection) -> Result<Self> {
        let proxy = Proxy::new(
            &conn,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.RemoteDesktop",
        )
        .await?;
        Ok(Self { conn: conn.clone(), proxy, operation_lock: Mutex::new(()) })
    }

    pub async fn create_session(&self, options: HashMap<&str, Value<'_>>) -> Result<OwnedObjectPath> {
        Ok(self.proxy.call("CreateSession", &(options)).await?)
    }

    pub async fn select_devices(&self, session_handle: &OwnedObjectPath, options: HashMap<&str, Value<'_>>) -> Result<OwnedObjectPath> {
        Ok(self.proxy.call("SelectDevices", &(session_handle, options)).await?)
    }

    pub async fn start(&self, session_handle: &OwnedObjectPath, parent_window: &str, options: HashMap<&str, Value<'_>>) -> Result<OwnedObjectPath> {
        Ok(self.proxy.call("Start", &(session_handle, parent_window, options)).await?)
    }

    pub async fn notify_pointer_motion_absolute(&self, session_handle: &OwnedObjectPath, options: HashMap<&str, Value<'_>>, stream: u32, x: f64, y: f64) -> Result<()> {
        let _guard = self.operation_lock.lock().await;
        Ok(self.proxy.call("NotifyPointerMotionAbsolute", &(session_handle, options, stream, x, y)).await?)
    }
    pub async fn notify_pointer_button(&self, session_handle: &OwnedObjectPath, options: HashMap<&str, Value<'_>>, button: i32, state: u32) -> Result<()> {
        let _guard = self.operation_lock.lock().await;
        Ok(self.proxy.call("NotifyPointerButton", &(session_handle, options, button, state)).await?)
    }
}

#[derive(Debug)]
pub struct RemoteDesktopPortal {
    inner: RemoteDesktopPortalProxy,
    session_handle: Option<OwnedObjectPath>,

    stream_id: u32
}

impl RemoteDesktopPortal {
    pub async fn new(connection: &Connection) -> Result<Self> {
        let inner = RemoteDesktopPortalProxy::new(connection).await?;
        Ok(Self { inner, session_handle: None, stream_id: 0 })
    }

    pub async fn create_session(&mut self, options: HashMap<&str, Value<'_>>) -> Result<()> {
        let handle = self.inner.create_session(options).await?;
        let handle = RequestPortalProxy::new(&self.inner.conn, handle).await?;
        let result = wait_for_portal_response(handle).await?;
        let session_handle_str = result.get("session_handle")
            .and_then(|v| Some(v.downcast_ref::<String>())).ok_or(Arc::new(std::io::Error::new(std::io::ErrorKind::Other, "Missing session_handle in response")))??;
        self.session_handle = Some(OwnedObjectPath::try_from(session_handle_str.as_str())?);
        Ok(())
    }

    pub async fn select_devices(&mut self, options: HashMap<&str, Value<'_>>) -> Result<()> {
        let session_handle = self.session_handle.as_ref()
            .ok_or(Arc::new(std::io::Error::new(std::io::ErrorKind::Other, "No session created")))?;
        let _h = self.inner.select_devices(session_handle, options).await?;
        Ok(())
    }

    pub async fn start(&mut self, parent_window: &str, options: HashMap<&str, Value<'_>>) -> Result<HashMap<String, OwnedValue>> {
        let session_handle = self.session_handle.as_ref()
            .ok_or(Arc::new(std::io::Error::new(std::io::ErrorKind::Other, "No session created")))?;
        let handle = self.inner.start(session_handle, parent_window, options).await?;
        let handle = RequestPortalProxy::new(&self.inner.conn, handle).await?;
        let result = wait_for_portal_response(handle).await?;

        Ok(result)
    }

    pub async fn notify_pointer_motion_absolute(&self, stream: u32, x: f64, y: f64, options: HashMap<&str, Value<'_>>) -> Result<()> {
        let session_handle = self.session_handle.as_ref()
            .ok_or(Arc::new(std::io::Error::new(std::io::ErrorKind::Other, "No session created")))?;
        Ok(self.inner.notify_pointer_motion_absolute(session_handle, options, stream, x as i32 as f64, y as i32 as f64).await?)
    }

    pub async fn notify_pointer_button(&self, button: i32, state: bool, options: HashMap<&str, Value<'_>>) -> Result<()> {
        let session_handle = self.session_handle.as_ref()
            .ok_or(Arc::new(std::io::Error::new(std::io::ErrorKind::Other, "No session created")))?;
        Ok(self.inner.notify_pointer_button(session_handle, options, button, state as u32).await?)
    }

    pub fn session_handle(&self) -> Option<&OwnedObjectPath> {
        self.session_handle.as_ref()
    }
}

impl CursorControl for RemoteDesktopPortal {
    fn move_cursor(&self, x: f64, y: f64) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(self.notify_pointer_motion_absolute(self.stream_id, x, y, HashMap::new()))
    }

    fn press_button(&self, button: u8) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(self.notify_pointer_button(button as i32, true, HashMap::new()))
    }

    fn release_button(&self, button: u8) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(self.notify_pointer_button(button as i32, false, HashMap::new()))
    }
}

#[derive(Debug)]
pub struct RequestPortalProxy {
    pub proxy: Proxy<'static>
}

impl RequestPortalProxy {
    pub async fn new(connection: &Connection, request_path: OwnedObjectPath) -> Result<Self> {
        let proxy = Proxy::new(
            &connection, 
            "org.freedesktop.portal.Desktop",
            request_path,
            "org.freedesktop.portal.Request"
        ).await?;
        Ok(Self { proxy })
    }

    pub async fn receive_signal_response(&self) -> Result<SignalStream<'static>> {
        Ok(self.proxy.receive_signal("Response").await?)
    }

    pub async fn close(&self) -> Result<()> {
        Ok(self.proxy.call("Close", &()).await?)
    }
}


pub async fn wait_for_portal_response(
    handle: RequestPortalProxy
) -> Result<HashMap<String, OwnedValue>> {
    let mut signal_stream = handle.receive_signal_response().await?;

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

#[derive(Debug)]
pub struct ScreenCastPortalProxy {
    pub conn: Connection,
    pub proxy: Proxy<'static>
}

impl ScreenCastPortalProxy {
    pub async fn new(conn: &Connection) -> Result<Self> {
        let proxy = Proxy::new(
            conn,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.ScreenCast",
        )
        .await?;
        Ok(Self { conn: conn.clone(), proxy })
    }

    pub async fn create_session(&self, options: HashMap<&str, Value<'_>>) -> Result<OwnedObjectPath> {
        Ok(self.proxy.call("CreateSession", &(options)).await?)
    }

    pub async fn select_sources(&self, session_handle: &OwnedObjectPath, options: HashMap<&str, Value<'_>>) -> Result<OwnedObjectPath> {
        Ok(self.proxy.call("SelectSources", &(session_handle, options)).await?)
    }

    pub async fn start(&self, session_handle: &OwnedObjectPath, parent_window: &str, options: HashMap<&str, Value<'_>>) -> Result<OwnedObjectPath> {
        Ok(self.proxy.call("Start", &(session_handle, parent_window, options)).await?)
    }

    pub async fn open_pipewire_remote(&self, session_handle: &OwnedObjectPath, options: HashMap<&str, Value<'_>>) -> Result<zvariant::OwnedFd> {
        Ok(self.proxy.call("OpenPipeWireRemote", &(session_handle, options)).await?)
    }
}


#[derive(Debug)]
pub struct ScreenCastPortal {
    inner: ScreenCastPortalProxy,
    session_handle: Option<OwnedObjectPath>,
    streams: Vec<PipeWireStream>,
}

#[derive(Debug)]
pub struct PipeWireStream {
    pub id: u32,
    pub position: Option<(i32, i32)>,
    pub size: Option<(i32, i32)>,
    pub source_type: Option<u32>,
    pub mapping_id: Option<String>,
    pub pipewire_serial: Option<i64>
}

impl ScreenCastPortal {
    pub async fn new(connection: &Connection) -> Result<Self> {
        let inner = ScreenCastPortalProxy::new(connection).await?;
        Ok(Self {
            inner,
            session_handle: None,
            streams: Vec::new(),
        })
    }

    pub async fn create_session(&mut self, options: HashMap<&str, Value<'_>>) -> Result<()> {
        let handle = self.inner.create_session(options).await?;
        let handle = RequestPortalProxy::new(&self.inner.conn, handle).await?;
        let result = wait_for_portal_response(handle).await?;
        let session_handle_str = result.get("session_handle")
            .and_then(|v| Some(v.downcast_ref::<String>())).ok_or(Arc::new(std::io::Error::new(std::io::ErrorKind::Other, "Missing session_handle in response")))??;
        self.session_handle = Some(OwnedObjectPath::try_from(session_handle_str.as_str())?);
        Ok(())
    }

    pub async fn select_sources(&mut self, options: HashMap<&str, Value<'_>>) -> Result<()> {
        let session_handle = self.session_handle.as_ref()
            .ok_or(Arc::new(std::io::Error::new(std::io::ErrorKind::Other, "No session created")))?;
        let _h = self.inner.select_sources(session_handle, options).await?;
        Ok(())
    }

    pub async fn start(&mut self, parent_window: &str, options: HashMap<&str, Value<'_>>) -> Result<()> {
        let session_handle = self.session_handle.as_ref()
            .ok_or(Arc::new(std::io::Error::new(std::io::ErrorKind::Other, "No session created")))?;
        let handle = self.inner.start(session_handle, parent_window, options).await?;
        let handle = RequestPortalProxy::new(&self.inner.conn, handle).await?;
        let result = wait_for_portal_response(handle).await?;

        self.process_start_result(result).await?;

        Ok(())
    }

    async fn process_start_result(&mut self, result: HashMap<String, OwnedValue>) -> Result<()> {
        let streams_value = result
            .get("streams")
            .ok_or_else(|| Box::new(std::io::Error::new(std::io::ErrorKind::Other, "no stream available")))?;

        // 1. Get the reference to the outer Array
        let stream_array = streams_value.downcast_ref::<Array>()?;

        let mut streams = Vec::new();

        // 2. Iterate through each element (which is a Structure representing the tuple (ua{sv}))
        for stream_item in stream_array.iter() {
            let stream_struct = stream_item.downcast_ref::<Structure>()?;
            let fields = stream_struct.fields();

            let id = fields
                .get(0)
                .ok_or_else(|| Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Missing id")))?;

            // field 0: The PipeWire Node ID (u32)
            let id: u32 = id.downcast_ref::<u32>()?;

            // field 1: The properties Dict (a{sv})
            let stream_dict = fields
                .get(1)
                .ok_or_else(|| Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Missing properties dict tuple element")))?
                .downcast_ref::<Dict>()?;

            // 3. Extract properties safely from the inner Dict
            let position: Option<(i32, i32)> = match stream_dict.get::<'_, String, Value>(&"position".to_string()) {
                Ok(Some(val)) => Some(val.downcast()?),
                _ => None,
            };

            let size: Option<(i32, i32)> = match stream_dict.get::<'_, String, Value>(&"size".to_string()) {
                Ok(Some(val)) => Some(val.downcast()?),
                _ => None,
            };

            let source_type: Option<u32> = match stream_dict.get::<'_, String, Value>(&"source_type".to_string()) {
                Ok(Some(val)) => Some(val.downcast()?),
                _ => None,
            };

            let mapping_id: Option<String> = match stream_dict.get::<'_, String, Value>(&"mapping_id".to_string()) {
                Ok(Some(val)) => Some(val.downcast()?),
                _ => None,
            };

            let pipewire_serial: Option<i64> = match stream_dict.get::<'_, String, Value>(&"pipewire-serial".to_string()) {
                Ok(Some(val)) => Some(val.downcast()?),
                _ => None,
            };

            streams.push(PipeWireStream {
                id,
                position,
                size,
                source_type,
                mapping_id,
                pipewire_serial
            });
        }

        self.streams = streams;

        Ok(())
    }

    pub async fn open_pipewire_remote(&self, options: HashMap<&str, Value<'_>>) -> Result<OwnedFd> {
        let session_handle = self.session_handle.as_ref()
            .ok_or(Arc::new(std::io::Error::new(std::io::ErrorKind::Other, "No session created")))?;
        Ok(self.inner.open_pipewire_remote(session_handle, options).await?.into())
    }

    pub fn session_handle(&self) -> Option<&OwnedObjectPath> {
        self.session_handle.as_ref()
    }
}

impl ScreenInfo for ScreenCastPortal {
    fn get_screen_id(&self) -> i32 {
        self.streams.first().map(|st| st.id as i32).unwrap_or(-1)
    }

    fn get_screen_dimensions(&self) -> (i32, i32) {
        self.streams.first().and_then(|st| st.size).unwrap_or((1920, 1080))
    }
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


#[derive(Debug)]
pub struct WaylandControlWrapper {
    remote_desktop_portal: Arc<RemoteDesktopPortal>,
    screen_cast_portal: Arc<ScreenCastPortal>
}

impl WaylandControlWrapper {
    pub async fn new() -> Result<Self> {
        let connection = Connection::session().await?;

        let mut remote_desktop_portal = RemoteDesktopPortal::new(&connection).await?;
        let mut screen_cast_portal = ScreenCastPortal::new(&connection).await?;
        
        let session_handle_token: String = generate_handle_token();

        remote_desktop_portal.create_session(vec![("session_handle_token", Value::from(session_handle_token))].into_iter().collect()).await?;
        screen_cast_portal.session_handle = remote_desktop_portal.session_handle.clone();

        // screen_cast_portal.create_session(vec![("session_handle_token", Value::from(session_handle_token.clone()))].into_iter().collect()).await?;
        screen_cast_portal.select_sources(HashMap::new()).await?;

        remote_desktop_portal.select_devices(vec![
            ("types", Value::U32(2)),
            ("multiple", Value::from(false))
        ].into_iter().collect()).await?;

        println!("Waiting for desktop portal interaction...");
        let res = remote_desktop_portal.start("", HashMap::new()).await?;

        screen_cast_portal.process_start_result(res).await?;
        
        let screen_cast_portal = Arc::new(screen_cast_portal);

        remote_desktop_portal.stream_id = screen_cast_portal.get_screen_id() as u32;
        
        let remote_desktop_portal = Arc::new(remote_desktop_portal);

        Ok(Self {
            remote_desktop_portal, screen_cast_portal
        })
    }
}

impl ControlWrapper for WaylandControlWrapper {
    fn get_cursor_control(&self) -> Arc<dyn CursorControl> {
        self.remote_desktop_portal.clone()
    }

    fn get_screen_info(&self) -> Arc<dyn crate::host_control::ScreenInfo> {
        self.screen_cast_portal.clone()
    }
}