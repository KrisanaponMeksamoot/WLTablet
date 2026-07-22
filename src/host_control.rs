use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[cfg(target_os = "linux")]
use x11rb::protocol::{
    xtest::fake_input,
    xproto::{warp_pointer, Screen}
};

#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::{
    Input::KeyboardAndMouse::{
        mouse_event, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
        MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_XDOWN,
        MOUSEEVENTF_XUP, VK_XBUTTON1, VK_XBUTTON2,
    },
    WindowsAndMessaging::{GetSystemMetrics, SetCursorPos, SM_CXSCREEN, SM_CYSCREEN},
};

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

#[cfg(target_os = "linux")]
#[derive(Debug)]
pub struct X11CursorControl {
    connection: x11rb::rust_connection::RustConnection,
    screen_num: usize,
    screen: Screen,
}

#[cfg(target_os = "linux")]
impl X11CursorControl {
    pub fn new() -> Result<Self> {
        let (connection, screen_num) = x11rb::connect(None)?;
        let screen = x11rb::connection::Connection::setup(&connection).roots[screen_num].clone();
        Ok(Self { connection, screen_num, screen })
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
pub struct X11ControlWrapper {
    inner: Arc<X11CursorControl>,
}

#[cfg(target_os = "linux")]
impl X11ControlWrapper {
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: Arc::new(X11CursorControl::new()?),
        })
    }
}

#[cfg(target_os = "linux")]
impl ControlWrapper for X11ControlWrapper {
    fn get_cursor_control(&self) -> Arc<dyn CursorControl> {
        self.inner.clone()
    }

    fn get_screen_info(&self) -> Arc<dyn ScreenInfo> {
        self.inner.clone()
    }
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
impl ScreenInfo for X11CursorControl {
    fn get_screen_dimensions(&self) -> (i32, i32) {
        let screen = &self.screen;
        (screen.width_in_pixels as i32, screen.height_in_pixels as i32)
    }
    
    fn get_screen_id(&self) -> i32 {
        self.screen_num as i32
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug, Default)]
pub struct WindowsCursorControl;

#[cfg(target_os = "windows")]
impl WindowsCursorControl {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    fn button_event(button: u8, press: bool) -> Result<(u32, i32)> {
        match (button, press) {
            (1, true) => Ok((MOUSEEVENTF_LEFTDOWN, 0)),
            (1, false) => Ok((MOUSEEVENTF_LEFTUP, 0)),
            (2, true) => Ok((MOUSEEVENTF_RIGHTDOWN, 0)),
            (2, false) => Ok((MOUSEEVENTF_RIGHTUP, 0)),
            (3, true) => Ok((MOUSEEVENTF_MIDDLEDOWN, 0)),
            (3, false) => Ok((MOUSEEVENTF_MIDDLEUP, 0)),
            (4, true) => Ok((MOUSEEVENTF_XDOWN, VK_XBUTTON1.into())),
            (4, false) => Ok((MOUSEEVENTF_XUP, VK_XBUTTON1.into())),
            (5, true) => Ok((MOUSEEVENTF_XDOWN, VK_XBUTTON2.into())),
            (5, false) => Ok((MOUSEEVENTF_XUP, VK_XBUTTON2.into())),
            _ => Err(format!("Unsupported mouse button: {button}").into()),
        }
    }
}

#[cfg(target_os = "windows")]
impl CursorControl for WindowsCursorControl {
    fn move_cursor(
        &self,
        x: f64,
        y: f64,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let success = unsafe { SetCursorPos(x.round() as i32, y.round() as i32) };

            if success == 0 {
                return Err(std::io::Error::last_os_error().into());
            }

            Ok(())
        })
    }

    fn press_button(
        &self,
        button: u8,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let (flags, data) = Self::button_event(button, true)?;

            unsafe {
                mouse_event(flags, 0, 0, data, 0);
            }

            Ok(())
        })
    }

    fn release_button(
        &self,
        button: u8,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let (flags, data) = Self::button_event(button, false)?;

            unsafe {
                mouse_event(flags, 0, 0, data, 0);
            }

            Ok(())
        })
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
pub struct WindowsControlWrapper {
    inner: Arc<WindowsCursorControl>,
}

#[cfg(target_os = "windows")]
impl WindowsControlWrapper {
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: Arc::new(WindowsCursorControl::new()?),
        })
    }
}

#[cfg(target_os = "windows")]
impl ControlWrapper for WindowsControlWrapper {
    fn get_cursor_control(&self) -> Arc<dyn CursorControl> {
        self.inner.clone()
    }

    fn get_screen_info(&self) -> Arc<dyn ScreenInfo> {
        self.inner.clone()
    }
}

#[cfg(target_os = "windows")]
impl ScreenInfo for WindowsCursorControl {
    fn get_screen_dimensions(&self) -> (i32, i32) {
        unsafe {
            (
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN),
            )
        }
    }

    fn get_screen_id(&self) -> i32 {
        0
    }
}
