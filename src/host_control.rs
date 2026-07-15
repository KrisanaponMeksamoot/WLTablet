use std::error::Error;
use std::pin::Pin;
use std::sync::Arc;

use x11rb::protocol::xproto::{Screen, warp_pointer};
use x11rb::protocol::xtest::fake_input;

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

#[derive(Debug)]
pub struct X11ControlWrapper {
    inner: Arc<X11CursorControl>
}

impl X11ControlWrapper {
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: Arc::new(X11CursorControl::new()?)
        })
    }
}

impl ControlWrapper for X11ControlWrapper {
    fn get_cursor_control(&self) -> Arc<dyn CursorControl> {
        self.inner.clone()
    }

    fn get_screen_info(&self) -> Arc<dyn ScreenInfo> {
        self.inner.clone()
    }
}

impl CursorControl for X11CursorControl {
    fn move_cursor(&self, x: f64, y: f64) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
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
}

impl ScreenInfo for X11CursorControl {
    fn get_screen_dimensions(&self) -> (i32, i32) {
        let screen = &self.screen;
        (screen.width_in_pixels as i32, screen.height_in_pixels as i32)
    }
    
    fn get_screen_id(&self) -> i32 {
        self.screen_num as i32
    }
}
