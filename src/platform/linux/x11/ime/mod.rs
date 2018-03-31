// Important: all XIM calls need to happen from the same thread!

mod inner;
mod context;
mod callbacks;

use std::mem;
use std::ptr;
use std::sync::Arc;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};

use super::{ffi, XConnection, XError};

use self::inner::ImeInner;
use self::context::{NewImeContextError, ImeContext};
use self::callbacks::*;

pub type ImeReceiver = Receiver<(ffi::Window, i16, i16)>;
pub type ImeSender = Sender<(ffi::Window, i16, i16)>;

#[derive(Debug)]
pub enum NewImeError {
    XError(XError),
    Null,
}

pub struct Ime {
    xconn: Arc<XConnection>,
    inner: Box<ImeInner>,
}

impl Ime {
    pub fn new(xconn: Arc<XConnection>) -> Result<Self, NewImeError> {
        let mut inner = Box::new(ImeInner::new(
            Arc::clone(&xconn),
            unsafe { mem::zeroed() },
            HashMap::new(),
        ));
        let client_data = Box::into_raw(inner);
        unsafe {
            let im = Ime::open_im(&xconn, client_data);
            inner = Box::from_raw(client_data);
            im
        }.map(|im| {
            (*inner).im = im;
            Ime {
                xconn,
                inner,
            }
        })
    }

    unsafe fn open_im(
        xconn: &Arc<XConnection>,
        client_data: *mut ImeInner,
    ) -> Result<ffi::XIM, NewImeError> {
        let im = (xconn.xlib.XOpenIM)(
            xconn.display,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        );
        if im.is_null() {
            return Err(NewImeError::Null);
        }

        let destroy_callback = ffi::XIMCallback {
            client_data: client_data as _,
            callback: Some(xim_destroy_callback),
        };
        xim_set_callback(
            &xconn,
            im,
            ffi::XNDestroyCallback_0.as_ptr() as *const _,
            // Make sure this isn't a leak...
            Box::into_raw(Box::new(destroy_callback)),
        ).map_err(NewImeError::XError)?;

        Ok(im)
    }

    pub fn is_destroyed(&self) -> bool {
        self.inner.destroyed
    }

    pub fn create_context(&mut self, window: ffi::Window) -> Result<(), NewImeContextError> {
        let context = if self.is_destroyed() {
            // Create empty entry in map, so that when IME is rebuilt, this window has a context.
            None
        } else {
            Some(unsafe { ImeContext::new(
                &self.inner.xconn,
                self.inner.im,
                window,
                None,
            ) }?)
        };
        self.inner.contexts.insert(window, context);
        Ok(())
    }

    pub fn destroy_context(&mut self, window: ffi::Window) -> Result<(), XError> {
        if self.is_destroyed() {
            return Ok(());
        }
        if let Some(Some(context)) = self.inner.contexts.remove(&window) {
            unsafe {
                (self.xconn.xlib.XDestroyIC)(context.ic);
            }
            self.xconn.check_errors()
        } else {
            Ok(())
        }
    }

    pub fn get_context(&self, window: ffi::Window) -> Option<ffi::XIC> {
        if self.is_destroyed() {
            return None;
        }
        if let Some(&Some(ref context)) = self.inner.contexts.get(&window) {
            Some(context.ic)
        } else {
            None
        }
    }

    pub fn focus(&mut self, window: ffi::Window) -> Result<(), XError> {
        if self.is_destroyed() {
            return Ok(());
        }
        if let Some(&mut Some(ref mut context)) = self.inner.contexts.get_mut(&window) {
            context.focus(&self.xconn)
        } else {
            Ok(())
        }
    }

    pub fn unfocus(&mut self, window: ffi::Window) -> Result<(), XError> {
        if self.is_destroyed() {
            return Ok(());
        }
        if let Some(&mut Some(ref mut context)) = self.inner.contexts.get_mut(&window) {
            context.unfocus(&self.xconn)
        } else {
            Ok(())
        }
    }

    pub fn send_xim_spot(&mut self, window: ffi::Window, x: i16, y: i16) {
        if self.is_destroyed() {
            return;
        }
        if let Some(&mut Some(ref mut context)) = self.inner.contexts.get_mut(&window) {
            context.send_xim_spot(&self.xconn, x as _, y as _);
        }
    }
}

impl Drop for Ime {
    fn drop(&mut self) {
        if !self.is_destroyed() {
            unsafe {
                for context in self.inner.contexts.values() {
                    if let &Some(ref context) = context {
                        (self.xconn.xlib.XDestroyIC)(context.ic);
                    }
                }
                (self.xconn.xlib.XCloseIM)(self.inner.im);
            }
            self.xconn.check_errors().expect("Failed to close input method");
        }
    }
}
