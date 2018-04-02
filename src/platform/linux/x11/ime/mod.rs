// Important: all XIM calls need to happen from the same thread!

mod inner;
mod input_method;
mod context;
mod callbacks;

use std::ptr;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::ffi::CStr;

use super::{ffi, util, XConnection, XError};

use self::inner::ImeInner;
use self::input_method::PotentialInputMethods;
use self::context::{NewImeContextError, ImeContext};
use self::callbacks::*;

pub type ImeReceiver = Receiver<(ffi::Window, i16, i16)>;
pub type ImeSender = Sender<(ffi::Window, i16, i16)>;

#[derive(Debug)]
pub enum ImeCreationError {
    XError(XError),
    OpenFailure(PotentialInputMethods),
}

impl From<XError> for ImeCreationError {
    fn from(err: XError) -> Self {
        ImeCreationError::XError(err)
    }
}

unsafe fn open_im(
    xconn: &Arc<XConnection>,
    locale: &CStr,
) -> Option<ffi::XIM> {
    (xconn.xlib.XSetLocaleModifiers)(locale.as_ptr());

    let im = (xconn.xlib.XOpenIM)(
        xconn.display,
        ptr::null_mut(),
        ptr::null_mut(),
        ptr::null_mut(),
    );

    if im.is_null() {
        None
    } else {
        Some(im)
    }
}

unsafe fn set_destroy_callback(
    xconn: &Arc<XConnection>,
    im: ffi::XIM,
    inner: &ImeInner,
) -> Result<(), XError> {
    xim_set_callback(
        &xconn,
        im,
        ffi::XNDestroyCallback_0.as_ptr() as *const _,
        &inner.destroy_callback as *const _ as *mut _,
    )
}

pub struct Ime {
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
            unsafe { set_destroy_callback(&inner.xconn, im.im, &*inner) }?;
            Ok(Ime { inner })
        } else {
            Err(ImeCreationError::OpenFailure(potential_input_methods))
        }
    }

    // HA HA HA
    fn get_xconn<'a, 'b>(&'a self) -> &'b Arc<XConnection> {
        unsafe { &*(&self.inner.xconn as *const _) }
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
        let xconn = self.get_xconn();
        if let Some(Some(context)) = self.inner.contexts.remove(&window) {
            unsafe {
                (xconn.xlib.XDestroyIC)(context.ic);
            }
            xconn.check_errors()
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
        let xconn = self.get_xconn();
        if let Some(&mut Some(ref mut context)) = self.inner.contexts.get_mut(&window) {
            context.focus(xconn)
        } else {
            Ok(())
        }
    }

    pub fn unfocus(&mut self, window: ffi::Window) -> Result<(), XError> {
        if self.is_destroyed() {
            return Ok(());
        }
        let xconn = self.get_xconn();
        if let Some(&mut Some(ref mut context)) = self.inner.contexts.get_mut(&window) {
            context.unfocus(xconn)
        } else {
            Ok(())
        }
    }

    pub fn send_xim_spot(&mut self, window: ffi::Window, x: i16, y: i16) {
        if self.is_destroyed() {
            return;
        }
        let xconn = self.get_xconn();
        if let Some(&mut Some(ref mut context)) = self.inner.contexts.get_mut(&window) {
            context.set_spot(xconn, x as _, y as _);
        }
    }
}

impl Drop for Ime {
    fn drop(&mut self) {
        if !self.is_destroyed() {
            let xconn = self.get_xconn();
            unsafe {
                for context in self.inner.contexts.values() {
                    if let &Some(ref context) = context {
                        (xconn.xlib.XDestroyIC)(context.ic);
                    }
                }
                (xconn.xlib.XCloseIM)(self.inner.im);
            }
            xconn.check_errors().expect("Failed to close input method");
        }
    }
}
