mod atom;
mod geometry;
mod hint;
mod input;
mod window_property;
mod wm;

pub use self::atom::*;
pub use self::geometry::*;
pub use self::hint::*;
pub use self::input::*;
pub use self::window_property::*;
pub use self::wm::*;

use std::mem;
use std::ptr;
use std::str;
use std::sync::Arc;
use std::ops::{Deref, DerefMut};
use std::os::raw::*;

use super::{ffi, XConnection, XError};

// This isn't actually the number of the bits in the format.
// X11 does a match on this value to determine which type to call sizeof on.
// Thus, we use 32 for c_long, since 32 maps to c_long which maps to 64.
// ...if that sounds confusing, then you know why this enum is here.
#[derive(Debug, Copy, Clone)]
pub enum Format {
    Char = 8,
    #[allow(dead_code)]
    Short = 16,
    Long = 32,
}

impl Format {
    pub fn from_format(format: usize) -> Option<Self> {
        match format {
            8 => Some(Format::Char),
            16 => Some(Format::Short),
            32 => Some(Format::Long),
            _ => None,
        }
    }

    pub fn is_same_size_as<T>(&self) -> bool {
        mem::size_of::<T>() == self.get_actual_size()
    }

    pub fn get_actual_size(&self) -> usize {
        match self {
            Format::Char => mem::size_of::<c_char>(),
            Format::Short => mem::size_of::<c_short>(),
            Format::Long => mem::size_of::<c_long>(),
        }
    }
}

pub struct XSmartPointer<'a, T> {
    xconn: &'a Arc<XConnection>,
    pub ptr: *mut T,
}

impl<'a, T> XSmartPointer<'a, T> {
    // You're responsible for only passing things to this that should be XFree'd.
    // Returns None if ptr is null.
    pub fn new(xconn: &'a Arc<XConnection>, ptr: *mut T) -> Option<Self> {
        if !ptr.is_null() {
            Some(XSmartPointer {
                xconn,
                ptr,
            })
        } else {
            None
        }
    }
}

impl<'a, T> Deref for XSmartPointer<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.ptr }
    }
}

impl<'a, T> DerefMut for XSmartPointer<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.ptr }
    }
}

impl<'a, T> Drop for XSmartPointer<'a, T> {
    fn drop(&mut self) {
        unsafe {
            (self.xconn.xlib.XFree)(self.ptr as *mut _);
        }
    }
}

// This is impoartant, so pay attention!
// Xlib has an output buffer, and tries to hide the async nature of X from you.
// This buffer contains the requests you make, and is flushed under various circumstances:
// 1. XPending, XNextEvent, and XWindowEvent flush "as needed"
// 2. XFlush explicitly flushes
// 3. XSync flushes and blocks until all requests are responded to
// 4. Calls that have a return dependent on a response (i.e. XGetWindowProperty) sync internally.
//    When in doubt, check the X11 source; if a function calls _XReply, it flushes and waits.
pub unsafe fn flush_requests(xconn: &Arc<XConnection>) -> Result<(), XError> {
    (xconn.xlib.XFlush)(xconn.display);
    // This isn't necessarily a useful time to check for errors (since our request hasn't
    // necessarily been processed yet)
    xconn.check_errors()
}

pub unsafe fn send_client_msg(
    xconn: &Arc<XConnection>,
    window: c_ulong,        // the window this is "about"; not necessarily this window
    target_window: c_ulong, // the window we're sending to
    message_type: ffi::Atom,
    event_mask: Option<c_long>,
    data: (c_long, c_long, c_long, c_long, c_long),
) -> Result<(), XError> {
    let mut event: ffi::XClientMessageEvent = mem::uninitialized();
    event.type_ = ffi::ClientMessage;
    event.display = xconn.display;
    event.window = window;
    event.message_type = message_type;
    event.format = Format::Long as c_int;
    event.data = ffi::ClientMessageData::new();
    event.data.set_long(0, data.0);
    event.data.set_long(1, data.1);
    event.data.set_long(2, data.2);
    event.data.set_long(3, data.3);
    event.data.set_long(4, data.4);

    let event_mask = event_mask.unwrap_or(ffi::NoEventMask);

    (xconn.xlib.XSendEvent)(
        xconn.display,
        target_window,
        ffi::False,
        event_mask,
        &mut event.into(),
    );

    // Since XSendEvent doesn't return, we need to flush!
    // (assuming we want it sent *now*)
    flush_requests(xconn)
}
