use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::control::{ControlWrapper, CursorControl, ScreenInfo};

use windows_sys::Win32::UI::{
    Input::KeyboardAndMouse::{
        mouse_event, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
        MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_XDOWN,
        MOUSEEVENTF_XUP, VK_XBUTTON1, VK_XBUTTON2,
    },
    WindowsAndMessaging::{GetSystemMetrics, SetCursorPos, SM_CXSCREEN, SM_CYSCREEN},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug, Default)]
pub struct WindowsCursorControl;

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

#[derive(Debug)]
pub struct WindowsControlWrapper {
    inner: Arc<WindowsCursorControl>,
}

impl WindowsControlWrapper {
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: Arc::new(WindowsCursorControl::new()?),
        })
    }
}

impl ControlWrapper for WindowsControlWrapper {
    fn get_cursor_control(&self) -> Arc<dyn CursorControl> {
        self.inner.clone()
    }

    fn get_screen_info(&self) -> Arc<dyn ScreenInfo> {
        self.inner.clone()
    }
}

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
