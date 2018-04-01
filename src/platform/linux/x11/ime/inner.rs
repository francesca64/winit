use std::mem;
use std::sync::Arc;
use std::collections::HashMap;

use super::{ffi, XConnection};

use super::context::ImeContext;

pub struct ImeInner {
    pub xconn: Arc<XConnection>,
    // Danger: this is initially zeroed!
    pub im: ffi::XIM,
    pub contexts: HashMap<ffi::Window, Option<ImeContext>>,
    // Danger: this is initially zeroed!
    pub destroy_callback: ffi::XIMCallback,
    // Indicates whether or not the the input method was destroyed on the server end
    // (i.e. if ibus/fcitx/etc. was terminated/restarted)
    pub destroyed: bool,
}

impl ImeInner {
    pub fn new(xconn: Arc<XConnection>) -> Self {
        ImeInner {
            xconn,
            im: unsafe { mem::zeroed() },
            contexts: HashMap::new(),
            destroy_callback: unsafe { mem::zeroed() },
            destroyed: false,
        }
    }
}
