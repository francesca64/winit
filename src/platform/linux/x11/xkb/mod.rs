mod state;

pub use self::state::*;

use std::mem;
use std::sync::Arc;
use std::collections::HashMap;

use x11_dl::xlib_xcb::xcb_connection_t;
use xkbcommon_dl::*;

use super::{ffi, util, XConnection, XError};
use events::ModifiersState;

pub fn xkb_keycode_from_x11_keycode(keycode: i32) -> xkb_keycode_t {
    // https://tronche.com/gui/x/xlib/input/keyboard-encoding.html
    debug_assert!(keycode >= 8 && keycode <= 255);
    keycode as u32
}

#[derive(Debug)]
pub enum XkbInitError {
    LibxkbcommonUnavailable,
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
pub struct Xkb {
    xconn: Arc<XConnection>,
    xcb_conn: *mut xcb_connection_t,
    pub event_code: u8,
    _core_device_id: i32,
    context: *mut xkb_context,
    keyboards: HashMap<i32, XkbState>,
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
        if XKBCOMMON_OPTION.is_none() || XKBCOMMON_X11_OPTION.is_none() {
            return Err(XkbInitError::LibxkbcommonUnavailable);
        }

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
            _core_device_id: core_device_id,
            context,
            keyboards: HashMap::new(),
        })
    }

    pub fn add_keyboard(&mut self, device_id: i32) -> Result<(), XkbStateInitError> {
        let state = unsafe {
            XkbState::new(&self.xconn, self.xcb_conn, self.context, device_id)
        }?;
        self.keyboards.insert(device_id, state);
        Ok(())
    }

    pub fn get_keysym(&self, device_id: i32, keycode: i32) -> Option<u32> {
        let keycode = xkb_keycode_from_x11_keycode(keycode);
        self.keyboards
            .get(&device_id)
            .map(|state| state.get_keysym(keycode))
    }

    pub fn get_modifiers(&self, device_id: i32) -> Option<ModifiersState> {
        self.keyboards
            .get(&device_id)
            .map(|state| state.get_modifiers())
    }

    pub fn update(
        &mut self,
        device_id: i32,
        depressed_mods: xkb_mod_mask_t,
        latched_mods: xkb_mod_mask_t,
        locked_mods: xkb_mod_mask_t,
        depressed_layout: i32,
        latched_layout: i32,
        locked_layout: i32,
    ) {
        if let Some(state) = self.keyboards.get_mut(&device_id) {
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
}
