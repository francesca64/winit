#![cfg(target_os = "macos")]

mod app;
mod app_delegate;
mod event_loop;
mod ffi;
mod monitor;
mod observer;
mod util;
mod view;
mod window;
mod window_delegate;

use std::{ops::Deref, sync::Arc};

use {
    event::DeviceId as RootDeviceId, window::{CreationError, WindowAttributes},
};
pub use self::{
    event_loop::{EventLoop, EventLoopWindowTarget, Proxy as EventLoopProxy},
    monitor::MonitorHandle,
    window::{
        Id as WindowId, PlatformSpecificWindowBuilderAttributes, UnownedWindow,
    },
};
use self::window_delegate::WindowDelegate;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId;

// Constant device ID; to be removed when if backend is updated to report real device IDs.
pub(crate) const DEVICE_ID: RootDeviceId = RootDeviceId(DeviceId);

pub struct Window {
    window: Arc<UnownedWindow>,
    // We keep this around so that it doesn't get dropped until the window does.
    _delegate: WindowDelegate,
}

impl Deref for Window {
    type Target = UnownedWindow;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &*self.window
    }
}

impl Window {
    pub fn new<T: 'static>(
        elw_target: &EventLoopWindowTarget<T>,
        attributes: WindowAttributes,
        pl_attribs: PlatformSpecificWindowBuilderAttributes,
    ) -> Result<Self, CreationError> {
        UnownedWindow::new(elw_target, attributes, pl_attribs)
            .map(|(window, _delegate)| {
                elw_target
                    .window_list
                    .lock()
                    .unwrap()
                    .insert_window(Arc::downgrade(&window));
                Window { window, _delegate }
            })
    }
}
