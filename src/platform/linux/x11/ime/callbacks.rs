use std::mem;
use std::ptr;
use std::sync::Arc;
use std::os::raw::c_char;

use super::{ffi, XConnection, XError};

use super::inner::ImeInner;
use super::context::ImeContext;

pub unsafe fn xim_set_callback(
    xconn: &Arc<XConnection>,
    xim: ffi::XIM,
    field: *const c_char,
    callback: *mut ffi::XIMCallback,
) -> Result<(), XError> {
    // It's advisable to wrap variadic FFI functions in our own functions, as we want to minimize
    // access that isn't type-checked.
    (xconn.xlib.XSetIMValues)(
        xim,
        field,
        callback,
        ptr::null_mut::<()>(),
    );
    xconn.check_errors()
}

pub unsafe fn set_destroy_callback(
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

// Attempt to replace current IM (which may or may not be presently valid) with a new one. This
// includes replacing all existing input contexts and free'ing resources as necessary. This only
// modifies existing state if all operations succeed.
// WARNING: at the time of writing, this comment is a bold-faced lie.
unsafe fn replace_im(inner: *mut ImeInner) {
    let xconn = &(*inner).xconn;
    let im = (*inner).potential_input_methods.open_im(xconn)
        .ok()
        .expect("Failed to reopen input method");
    println!("IM {:?}", im);
    println!("(POTENTIAL {:#?})", (*inner).potential_input_methods);
    (*inner).im = im.im;
    for (window, old_context) in (*inner).contexts.iter_mut() {
        let spot = old_context.as_ref().map(|context| context.ic_spot);
        let new_context = ImeContext::new(
            xconn,
            im.im,
            *window,
            spot,
        ).expect("Failed to reinitialize input context");
        let _ = mem::replace(old_context, Some(new_context));
    }
    (*inner).destroyed = false;
}

// This callback is triggered when a new input method using the same locale modifiers becomes
// available. In other words, if ibus/fcitx/etc. is restarted, this responds to that. Note that if
// the program is started while the input method isn't running, then this won't be triggered by
// the input method starting, since we won't be using the respective locale modifier.
pub unsafe extern fn xim_instantiate_callback(
    _display: *mut ffi::Display,
    client_data: ffi::XPointer,
    // This field is unsupplied
    _call_data: ffi::XPointer,
) {
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
        replace_im(inner);
        // Allow failure if non-destroyed fallback is present
        // otherwise panic
        set_destroy_callback(xconn, (*inner).im, &*inner)
            .expect("Failed to set input method destruction callback");
    }
}

// This callback is triggered when the input method is closed on the server end. When this
// happens, XCloseIM/XDestroyIC doesn't need to be called, as the resources have already been free'd
// (attempting to do so causes a freeze)
pub unsafe extern fn xim_destroy_callback(
    _xim: ffi::XIM,
    client_data: ffi::XPointer,
    // This field is unsupplied
    _call_data: ffi::XPointer,
) {
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
        // Attempt to open fallback input method
        // The IM+ICs we open here get leaked!
        replace_im(inner);
        // This needs to have a destroy callback too to ensure we don't try to free anything we
        // shouldn't
    }
}
