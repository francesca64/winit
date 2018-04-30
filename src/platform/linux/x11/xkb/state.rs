use std::sync::Arc;
use std::os::raw::{c_int, c_uint};

use x11_dl::xlib_xcb::xcb_connection_t;
use xkbcommon_dl::*;

use events::ModifiersState;

use super::*;

#[derive(Debug)]
pub enum XkbStateInitError {
    KeymapIsNull,
    StateIsNull,
    FailedToSelectEvents(XError),
    XkbExtNotInitialized,
}

impl From<XError> for XkbStateInitError {
    fn from(e: XError) -> Self {
        XkbStateInitError::FailedToSelectEvents(e)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ModStatus {
    Invalid = -1,
    Inactive = 0,
    Active = 1,
}

impl From<c_int> for ModStatus {
    fn from(i: c_int) -> Self {
        if i < 0 {
            ModStatus::Invalid
        } else if i > 0 {
            ModStatus::Active
        } else {
            ModStatus::Inactive
        }
    }
}

#[derive(Debug)]
pub struct XkbState {
    keymap: *mut xkb_keymap,
    state: *mut xkb_state,
}

impl Drop for XkbState {
    fn drop(&mut self) {
        unsafe {
            (XKBCOMMON_HANDLE.xkb_state_unref)(self.state);
            (XKBCOMMON_HANDLE.xkb_keymap_unref)(self.keymap);
        }
    }
}

impl XkbState {
    pub unsafe fn new(
        xconn: &Arc<XConnection>,
        xcb_conn: *mut xcb_connection_t,
        context: *mut xkb_context,
        device_id: i32
    ) -> Result<Self, XkbStateInitError> {
        let keymap = (XKBCOMMON_X11_HANDLE.xkb_x11_keymap_new_from_device)(
            context,
            xcb_conn,
            device_id,
            xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
        );
        if keymap.is_null() {
            return Err(XkbStateInitError::KeymapIsNull);
        }

        let state = (XKBCOMMON_X11_HANDLE.xkb_x11_state_new_from_device)(
            keymap,
            xcb_conn,
            device_id,
        );
        if state.is_null() {
            return Err(XkbStateInitError::StateIsNull);
        }

        let mask = ffi::XkbNewKeyboardNotifyMask
            | ffi::XkbMapNotifyMask
            | ffi::XkbStateNotifyMask;
        util::select_xkb_events(
            xconn,
            device_id as c_uint,
            mask,
        ).ok_or(XkbStateInitError::XkbExtNotInitialized)?.queue();

        // Don't return an XkbState if anything's wrong
        util::sync_with_server(xconn)?;

        Ok(XkbState {
            keymap,
            state,
        })
    }

    pub fn get_keysym(&self, keycode: xkb_keycode_t) -> xkb_keysym_t {
        unsafe {
            (XKBCOMMON_HANDLE.xkb_state_key_get_one_sym)(self.state, keycode)
        }
    }

    fn get_modifier(&self, modkey: &[u8]) -> ModStatus {
        unsafe {
            (XKBCOMMON_HANDLE.xkb_state_mod_name_is_active)(
                self.state,
                modkey as *const _ as *const i8,
                xkb_state_component::XKB_STATE_MODS_EFFECTIVE,
            )
        }.into()
    }

    pub fn get_modifiers(&self) -> ModifiersState {
        let alt = self.get_modifier(XKB_MOD_NAME_ALT) == ModStatus::Active;
        let shift = self.get_modifier(XKB_MOD_NAME_SHIFT) == ModStatus::Active;
        let ctrl = self.get_modifier(XKB_MOD_NAME_CTRL) == ModStatus::Active;
        let logo = self.get_modifier(XKB_MOD_NAME_LOGO) == ModStatus::Active;
        ModifiersState {
            alt,
            shift,
            ctrl,
            logo,
        }
    }

    pub fn update(
        &mut self,
        depressed_mods: xkb_mod_mask_t,
        latched_mods: xkb_mod_mask_t,
        locked_mods: xkb_mod_mask_t,
        depressed_layout: xkb_layout_index_t,
        latched_layout: xkb_layout_index_t,
        locked_layout: xkb_layout_index_t,
    ) {
        unsafe {
            (XKBCOMMON_HANDLE.xkb_state_update_mask)(
                self.state,
                depressed_mods,
                latched_mods,
                locked_mods,
                depressed_layout,
                latched_layout,
                locked_layout,
            );
        }
    }
}
