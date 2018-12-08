#![cfg(target_os = "macos")]

mod event_loop;
mod ffi;
mod monitor;
mod util;
mod view;
mod window;
mod window_delegate;

use std::{ops::Deref, sync::Arc};

pub use self::{
    event,
    event_loop::{EventLoopWindowTarget, Proxy as EventLoopProxy},
    monitor::MonitorHandle,
    window::{
        Id as WindowId, PlatformSpecificWindowBuilderAttributes,
        UnownedWindow, WindowDelegate,
    },
};
use window::{CreationError, WindowAttributes};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId;

// Constant device ID; to be removed when if backend is updated to report real device IDs.
pub(crate) const DEVICE_ID: event::DeviceId = event::DeviceId(DeviceId);

pub struct Window {
    window: Arc<UnownedWindow>,
    delegate: WindowDelegate,
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
        event_loop: &EventLoopWindowTarget<T>,
        attributes: WindowAttributes,
        pl_attribs: PlatformSpecificWindowBuilderAttributes,
    ) -> Result<Self, CreationError> {
        UnownedWindow::new(Arc::downgrade(&event_loop.shared), attributes, pl_attribs)
            .map(Arc::new)
            .map(|(window, delegate)| {
                event_loop.shared.windows
                    .lock()
                    .unwrap()
                    .push(Arc::downgrade(&window));
                Window { window, delegate }
            })
    }
}
