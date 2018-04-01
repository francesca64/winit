use std::ptr;
use std::sync::Arc;
use std::os::raw::c_short;

use super::{ffi, XConnection, XError};

#[derive(Debug)]
pub enum NewImeContextError {
    XError(XError),
    Null,
}

pub struct ImeContext {
    pub ic: ffi::XIC,
    pub ic_spot: ffi::XPoint,
}

impl ImeContext {
    pub unsafe fn new(
        xconn: &Arc<XConnection>,
        im: ffi::XIM,
        window: ffi::Window,
        ic_spot: Option<ffi::XPoint>,
    ) -> Result<Self, NewImeContextError> {
        let ic = if let Some(ic_spot) = ic_spot {
            ImeContext::create_ic_with_spot(xconn, im, window, ic_spot)
        } else {
            ImeContext::create_ic(xconn, im, window)
        };

        let ic = ic.ok_or(NewImeContextError::Null)?;
        xconn.check_errors().map_err(NewImeContextError::XError)?;

        Ok(ImeContext {
            ic,
            ic_spot: ic_spot.unwrap_or_else(|| ffi::XPoint { x: 0, y: 0 }),
        })
    }

    unsafe fn create_ic(
        xconn: &Arc<XConnection>,
        im: ffi::XIM,
        window: ffi::Window,
    ) -> Option<ffi::XIC> {
        let ic = (xconn.xlib.XCreateIC)(
            im,
            ffi::XNInputStyle_0.as_ptr() as *const _,
            ffi::XIMPreeditNothing | ffi::XIMStatusNothing,
            ffi::XNClientWindow_0.as_ptr() as *const _,
            window,
            ptr::null_mut::<()>(),
        );
        if ic.is_null() {
            None
        } else {
            Some(ic)
        }
    }

    unsafe fn create_ic_with_spot(
        xconn: &Arc<XConnection>,
        im: ffi::XIM,
        window: ffi::Window,
        ic_spot: ffi::XPoint,
    ) -> Option<ffi::XIC> {
        let preedit_attr = (xconn.xlib.XVaCreateNestedList)(
            0,
            ffi::XNSpotLocation_0.as_ptr() as *const _,
            &ic_spot,
            ptr::null_mut::<()>(),
        );
        let ic = (xconn.xlib.XCreateIC)(
            im,
            ffi::XNInputStyle_0.as_ptr() as *const _,
            ffi::XIMPreeditNothing | ffi::XIMStatusNothing,
            ffi::XNClientWindow_0.as_ptr() as *const _,
            window,
            ffi::XNPreeditAttributes_0.as_ptr() as *const _,
            preedit_attr,
            ptr::null_mut::<()>(),
        );
        (xconn.xlib.XFree)(preedit_attr);
        if ic.is_null() {
            None
        } else {
            Some(ic)
        }
    }

    pub fn focus(&self, xconn: &Arc<XConnection>) -> Result<(), XError> {
        unsafe {
            (xconn.xlib.XSetICFocus)(self.ic);
        }
        xconn.check_errors()
    }

    pub fn unfocus(&self, xconn: &Arc<XConnection>) -> Result<(), XError> {
        unsafe {
            (xconn.xlib.XUnsetICFocus)(self.ic);
        }
        xconn.check_errors()
    }

    pub fn set_spot(&mut self, xconn: &Arc<XConnection>, x: c_short, y: c_short) {
        let nspot = ffi::XPoint { x, y };
        if self.ic_spot.x == x && self.ic_spot.y == y {
            return;
        }
        self.ic_spot = nspot;

        unsafe {
            let preedit_attr = (xconn.xlib.XVaCreateNestedList)(
                0,
                ffi::XNSpotLocation_0.as_ptr() as *const _,
                &nspot,
                ptr::null_mut::<()>(),
            );
            (xconn.xlib.XSetICValues)(
                self.ic,
                ffi::XNPreeditAttributes_0.as_ptr() as *const _,
                preedit_attr,
                ptr::null_mut::<()>(),
            );
            (xconn.xlib.XFree)(preedit_attr);
        }
    }
}
