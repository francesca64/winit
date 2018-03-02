mod state;
mod compose;

pub use self::state::*;
pub use self::compose::*;

use std::mem;
use std::sync::Arc;
use std::os::raw::c_uint;
use std::collections::HashMap;

use x11_dl::xlib_xcb::xcb_connection_t;
use xkbcommon_dl::*;

use super::{ffi, XConnection};
use super::events::keysym_to_element;
use events::{ElementState, ModifiersState, VirtualKeyCode};

pub fn scancode_from_keycode(keycode: i32) -> u32 {
    let scancode = keycode - 8;
    assert!(scancode >= 0);
    scancode as u32
}

pub fn xkb_keycode_from_x11_keycode(keycode: i32) -> xkb_keycode_t {
    // https://tronche.com/gui/x/xlib/input/keyboard-encoding.html
    assert!(keycode >= 8 && keycode <= 255);
    keycode as u32
}

#[derive(Debug)]
pub enum XkbInitError {
    ExtensionSetupFailed,
    ContextIsNull,
    CoreDeviceIdInvalid,
    StateInitFailed(XkbStateInitError),
}

impl From<XkbStateInitError> for XkbInitError {
    fn from(e: XkbStateInitError) -> Self {
        XkbInitError::StateInitFailed(e)
    }
}

#[derive(Debug)]
pub struct XkbReturnData {
    pub scancode: u32,
    pub virtual_keycode: Option<VirtualKeyCode>,
    pub modifiers: ModifiersState,
    pub utf8: Option<String>,
}

pub struct Xkb {
    xconn: Arc<XConnection>,
    xcb_conn: *mut xcb_connection_t,
    pub event_code: u8,
    //pub core_device_id: i32,
    context: *mut xkb_context,
    states: HashMap<i32, XkbState>,
}

impl Drop for Xkb {
    fn drop(&mut self) {
        unsafe {
            (XKBCOMMON_HANDLE.xkb_context_unref)(self.context);
        }
    }
}

impl Xkb {
    pub unsafe fn new(xconn: &Arc<XConnection>) -> Result<Self, XkbInitError> {
        let xcb_conn = (xconn.xlib_xcb.XGetXCBConnection)(xconn.display);

        let flags = xkb_x11_setup_xkb_extension_flags::XKB_X11_SETUP_XKB_EXTENSION_NO_FLAGS;
        let mut major_ver_out: u16 = mem::uninitialized();
        let mut minor_ver_out: u16 = mem::uninitialized();
        let mut base_event_out: u8 = mem::uninitialized();
        let mut base_error_out: u8 = mem::uninitialized();
        let ext_status = (XKBCOMMON_X11_HANDLE.xkb_x11_setup_xkb_extension)(
            xcb_conn,
            1, // major version
            0, // minor version
            flags,
            &mut major_ver_out,
            &mut minor_ver_out,
            &mut base_event_out,
            &mut base_error_out,
        );
        if ext_status == 0 {
            return Err(XkbInitError::ExtensionSetupFailed);
        }

        let mut supported_out: ffi::Bool = mem::uninitialized();
        (xconn.xlib.XkbSetDetectableAutoRepeat)(
            xconn.display,
            ffi::True,
            &mut supported_out,
        );

        let context = (XKBCOMMON_HANDLE.xkb_context_new)(xkb_context_flags::XKB_CONTEXT_NO_FLAGS);
        if context.is_null() {
            return Err(XkbInitError::ContextIsNull);
        }

        let core_device_id = (XKBCOMMON_X11_HANDLE.xkb_x11_get_core_keyboard_device_id)(xcb_conn);
        if core_device_id == -1 {
            return Err(XkbInitError::CoreDeviceIdInvalid);
        }

        Ok(Xkb {
            xconn: Arc::clone(xconn),
            xcb_conn,
            event_code: base_event_out,
            //core_device_id,
            context,
            states: HashMap::new(),
        })
    }

    pub unsafe fn create_state(&mut self, device_id: i32) -> Result<(), XkbStateInitError> {
        let state = XkbState::new(&self.xconn, self.xcb_conn, self.context, device_id)?;
        self.states.insert(device_id, state);
        Ok(())
    }

    pub unsafe fn get_or_create_state(
        &mut self,
        device_id: i32,
    ) -> Result<&mut XkbState, XkbStateInitError> {
        if !self.states.contains_key(&device_id) {
            self.create_state(device_id)?;
        }
        Ok(self.states.get_mut(&device_id).unwrap())
    }

    unsafe fn get_data_internal(
        &mut self,
        device_id: i32,
        keycode: i32,
        element_state: ElementState,
        raw: bool,
    ) -> XkbReturnData {
        let scancode = scancode_from_keycode(keycode);
        let keycode = xkb_keycode_from_x11_keycode(keycode);

        let state = self.get_or_create_state(device_id)
            .expect("Failed to create XkbState while getting data");
        let keysym = state.get_keysym(keycode, element_state, raw);
        let virtual_keycode = keysym_to_element(keysym as c_uint);
        let modifiers = state.get_modifiers();
        let utf8 = if !raw {
            state.get_utf8(keycode, element_state)
        } else {
            None
        };

        XkbReturnData {
            scancode,
            virtual_keycode,
            modifiers,
            utf8,
        }
    }

    pub unsafe fn get_data(
        &mut self,
        device_id: i32,
        keycode: i32,
        element_state: ElementState,
    ) -> XkbReturnData {
        self.get_data_internal(device_id, keycode, element_state, false)
    }

    /// Just like get_data, but doesn't influence the Compose state machine or return a UTF8 value.
    pub unsafe fn get_data_raw(
        &mut self,
        device_id: i32,
        keycode: i32,
        element_state: ElementState,
    ) -> XkbReturnData {
        self.get_data_internal(device_id, keycode, element_state, true)
    }

    pub unsafe fn update(
        &mut self,
        device_id: i32,
        depressed_mods: xkb_mod_mask_t,
        latched_mods: xkb_mod_mask_t,
        locked_mods: xkb_mod_mask_t,
        depressed_layout: i32,
        latched_layout: i32,
        locked_layout: i32,
    ) {
        let state = self.get_or_create_state(device_id)
            .expect("Failed to create XkbState while updating state");
        state.update(
            depressed_mods,
            latched_mods,
            locked_mods,
            depressed_layout as _,
            latched_layout as _,
            locked_layout as _,
        );
    }
}
