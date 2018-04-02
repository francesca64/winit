// Important: all XIM calls need to happen from the same thread!

mod inner;
mod input_method;
mod context;
mod callbacks;

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

use super::{ffi, util, XConnection, XError};

use self::inner::ImeInner;
use self::input_method::PotentialInputMethods;
use self::context::{ImeContextCreationError, ImeContext};
use self::callbacks::*;

pub type ImeReceiver = Receiver<(ffi::Window, i16, i16)>;
pub type ImeSender = Sender<(ffi::Window, i16, i16)>;

#[derive(Debug)]
pub enum ImeCreationError {
    OpenFailure(PotentialInputMethods),
    SetDestroyCallback(XError),
}

pub struct Ime {
    xconn: Arc<XConnection>,
    // The actual meat of this struct is boxed away, since it needs to have a fixed location in
    // memory so we can pass a pointer to it around.
    inner: Box<ImeInner>,
}

impl Ime {
    pub fn new(xconn: Arc<XConnection>) -> Result<Self, ImeCreationError> {
        let mut potential_input_methods = PotentialInputMethods::new(&xconn);
        let im = potential_input_methods.open_im(&xconn);
        println!("IM {:?}", im);
        println!("(POTENTIAL {:#?})", potential_input_methods);
        if let Some(im) = im.ok() {
            let mut inner = {
                let mut inner = Box::new(ImeInner::new(
                    xconn,
                    im.im,
                    potential_input_methods,
                ));
                let client_data = Box::into_raw(inner);
                let destroy_callback = ffi::XIMCallback {
                    client_data: client_data as _,
                    callback: Some(xim_destroy_callback),
                };
                inner = unsafe { Box::from_raw(client_data) };
                inner.destroy_callback = destroy_callback;
                inner
            };
            unsafe { set_destroy_callback(&inner.xconn, im.im, &*inner) }
                .map_err(ImeCreationError::SetDestroyCallback)?;
            Ok(Ime {
                xconn: Arc::clone(&inner.xconn),
                inner,
            })
        } else {
            Err(ImeCreationError::OpenFailure(potential_input_methods))
        }
    }

    pub fn is_destroyed(&self) -> bool {
        self.inner.destroyed
    }

    pub fn create_context(&mut self, window: ffi::Window) -> Result<(), ImeContextCreationError> {
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

    // For both focus and unfocus:
    // Ok(_) indicates that nothing went wrong internally
    // Ok(true) indicates that the action was actually performed
    // Ok(false) indicates that the action is not presently applicable
    pub fn focus(&mut self, window: ffi::Window) -> Result<bool, XError> {
        if self.is_destroyed() {
            return Ok(false);
        }
        if let Some(&mut Some(ref mut context)) = self.inner.contexts.get_mut(&window) {
            context.focus(&self.xconn).map(|_| true)
        } else {
            Ok(false)
        }
    }

    pub fn unfocus(&mut self, window: ffi::Window) -> Result<bool, XError> {
        if self.is_destroyed() {
            return Ok(false);
        }
        if let Some(&mut Some(ref mut context)) = self.inner.contexts.get_mut(&window) {
            context.unfocus(&self.xconn).map(|_| true)
        } else {
            Ok(false)
        }
    }

    pub fn send_xim_spot(&mut self, window: ffi::Window, x: i16, y: i16) {
        if self.is_destroyed() {
            return;
        }
        if let Some(&mut Some(ref mut context)) = self.inner.contexts.get_mut(&window) {
            context.set_spot(&self.xconn, x as _, y as _);
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
