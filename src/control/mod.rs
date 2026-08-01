use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod x11;
#[cfg(target_os = "linux")]
pub mod xdg_desktop_portal;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub trait CursorControl: Send + Sync {
    fn move_cursor(&self, x: f64, y: f64) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
    fn press_button(&self, button: u8) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
    fn release_button(&self, button: u8) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
}

pub trait ScreenInfo: Send + Sync {
    fn get_screen_id(&self) -> i32;
    fn get_screen_dimensions(&self) -> (i32, i32);
}

pub trait ControlWrapper: Send + Sync {
    fn get_cursor_control(&self) -> Arc<dyn CursorControl>;
    fn get_screen_info(&self) -> Arc<dyn ScreenInfo>;
}

pub async fn get_control_wrapper() -> Result<Arc<dyn ControlWrapper>> {
    #[cfg(target_os = "linux")]
    let control_wrapper: Arc<dyn ControlWrapper> = {
        let session_type = std::env::var("XDG_SESSION_TYPE")?;
        println!("Detected {session_type}");
        match session_type.as_str() {
            "wayland" => Arc::new(xdg_desktop_portal::WaylandControlWrapper::new().await?),
            "x11" => Arc::new(x11::X11ControlWrapper::new()?),
            _ => return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Unsupported XDG_SESSION_TYPE")))
        }
    };

    #[cfg(target_os = "windows")]
    let control_wrapper: Arc<dyn ControlWrapper> = {
        println!("Detected Windows, using Windows cursor control");
        Arc::new(windows::WindowsControlWrapper::new()?)
    };

    Ok(control_wrapper)
}