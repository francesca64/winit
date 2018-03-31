use std::sync::Arc;
use std::collections::HashMap;

use super::{ffi, XConnection};

use super::context::ImeContext;

pub struct ImeInner {
    pub xconn: Arc<XConnection>,
    // Danger: this value is initially zeroed!
    pub im: ffi::XIM,
    pub contexts: HashMap<ffi::Window, Option<ImeContext>>,
    // Indicates whether or not the the input method was destroyed on the server end
    // (i.e. if ibus/etc. was terminated/restarted)
    pub destroyed: bool,
}

impl ImeInner {
    pub fn new(
        xconn: Arc<XConnection>,
        im: ffi::XIM,
        contexts: HashMap<ffi::Window, Option<ImeContext>>,
    ) -> Self {
        ImeInner {
            xconn,
            im,
            contexts,
            destroyed: false,
        }
    }
}
