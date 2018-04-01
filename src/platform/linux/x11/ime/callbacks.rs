use std::mem;
use std::ptr;
use std::sync::Arc;
use std::os::raw::c_char;

use super::{ffi, XConnection, XError};

use super::Ime;
use super::inner::ImeInner;
use super::context::ImeContext;

pub unsafe fn xim_set_callback(
    xconn: &Arc<XConnection>,
    xim: ffi::XIM,
    field: *const c_char,
    callback: *mut ffi::XIMCallback,
) -> Result<(), XError> {
    (xconn.xlib.XSetIMValues)(
        xim,
        field,
        callback,
        ptr::null_mut::<()>(),
    );
    xconn.check_errors()
}

pub unsafe extern fn xim_instantiate_callback(
    _display: *mut ffi::Display,
    client_data: ffi::XPointer,
    // This field is unsupplied
    _call_data: ffi::XPointer,
) {
    println!("INSTANTIATE=XIM");
    let inner: *mut ImeInner = client_data as _;
    if !client_data.is_null() {
        let xconn = &(*inner).xconn;
        (xconn.xlib.XUnregisterIMInstantiateCallback)(
            xconn.display,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            Some(xim_instantiate_callback),
            client_data,
        );
        let im = Ime::open_im(xconn, &*inner)
            .expect("Failed to reopen input method");
        (*inner).im = im;
        for (window, old_context) in (*inner).contexts.iter_mut() {
            let spot = old_context.as_ref().map(|context| context.ic_spot);
            let new_context = ImeContext::new(
                xconn,
                im,
                *window,
                spot,
            ).expect("Failed to reinitialize input context");
            let _ = mem::replace(old_context, Some(new_context));
        }
        (*inner).destroyed = false;
    }
}

pub unsafe extern fn xim_destroy_callback(
    _xim: ffi::XIM,
    client_data: ffi::XPointer,
    // This field is unsupplied
    _call_data: ffi::XPointer,
) {
    println!("DESTROYED=XIM");
    let inner: *mut ImeInner = client_data as _;
    if !inner.is_null() {
        (*inner).destroyed = true;
        let xconn = &(*inner).xconn;
        (xconn.xlib.XRegisterIMInstantiateCallback)(
            xconn.display,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            Some(xim_instantiate_callback),
            client_data,
        );
    }
}
