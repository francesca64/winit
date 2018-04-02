use std::mem;
use std::sync::Arc;
use std::collections::HashMap;

use super::{ffi, XConnection};

use super::input_method::PotentialInputMethods;
use super::context::ImeContext;

pub struct ImeInner {
    pub xconn: Arc<XConnection>,
    pub im: ffi::XIM,
    pub potential_input_methods: PotentialInputMethods,
    pub contexts: HashMap<ffi::Window, Option<ImeContext>>,
    // Danger: this is initially zeroed!
    pub destroy_callback: ffi::XIMCallback,
    // Indicates whether or not the the input method was destroyed on the server end
    // (i.e. if ibus/fcitx/etc. was terminated/restarted)
    pub destroyed: bool,
}

impl ImeInner {
    pub fn new(
        xconn: Arc<XConnection>,
        im: ffi::XIM,
        potential_input_methods: PotentialInputMethods,
    ) -> Self {
        ImeInner {
            xconn,
            im,
            potential_input_methods,
            contexts: HashMap::new(),
            destroy_callback: unsafe { mem::zeroed() },
            destroyed: false,
        }
    }
}
