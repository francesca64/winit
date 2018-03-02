use std::ptr;
use std::sync::Arc;
use std::os::raw::{c_int, c_uint};

use x11_dl::xlib_xcb::xcb_connection_t;
use xkbcommon_dl::*;

use events::{ElementState, ModifiersState};

use super::*;
use super::super::{ffi, XConnection, XError};

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

pub struct XkbState {
    keymap: *mut xkb_keymap,
    state: *mut xkb_state,
    compose: Option<XkbCompose>,
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

        let state =
            (XKBCOMMON_X11_HANDLE.xkb_x11_state_new_from_device)(keymap, xcb_conn, device_id);
        if state.is_null() {
            return Err(XkbStateInitError::StateIsNull);
        }

        // Select Xkb events
        let mask = ffi::XkbNewKeyboardNotifyMask
            | ffi::XkbMapNotifyMask
            | ffi::XkbStateNotifyMask;
        let status = (xconn.xlib.XkbSelectEvents)(
            xconn.display,
            device_id as c_uint,
            mask,
            mask,
        );

        xconn.check_errors()?;

        if status == ffi::False {
            return Err(XkbStateInitError::XkbExtNotInitialized);
        }

        // Compose is an optional feature, so don't sweat it if we can't initialize it.
        let compose = XkbCompose::new(context).ok();

        Ok(XkbState {
            keymap,
            state,
            compose,
        })
    }

    unsafe fn compose_status_check(&mut self, status: xkb_compose_status) -> bool {
        if let Some(ref mut compose) = self.compose {
            compose.compose_status == status
        } else {
            false
        }
    }

    pub unsafe fn is_composing(&mut self) -> bool {
        self.compose_status_check(xkb_compose_status::XKB_COMPOSE_COMPOSING)
    }

    pub unsafe fn is_composed(&mut self) -> bool {
        self.compose_status_check(xkb_compose_status::XKB_COMPOSE_COMPOSED)
    }

    pub unsafe fn feed_compose(&mut self, keysym: xkb_keysym_t) {
        if let Some(ref mut compose) = self.compose {
            compose.feed_keysym(keysym);
        }
    }

    unsafe fn get_keysym_direct(&mut self, keycode: xkb_keycode_t) -> xkb_keysym_t {
        (XKBCOMMON_HANDLE.xkb_state_key_get_one_sym)(self.state, keycode)
    }

    pub unsafe fn get_keysym(
        &mut self,
        keycode: xkb_keycode_t,
        element_state: ElementState,
        bypass_compose: bool,
    ) -> xkb_keysym_t {
        let keysym = self.get_keysym_direct(keycode);
        if !bypass_compose {
            if element_state == ElementState::Pressed {
                self.feed_compose(keysym);
            }
            if self.is_composed() {
                self.compose.as_mut().unwrap().get_keysym()
            } else {
                keysym
            }
        } else {
            keysym
        }
    }

    unsafe fn get_modifier(&mut self, modkey: &[u8]) -> ModStatus {
        (XKBCOMMON_HANDLE.xkb_state_mod_name_is_active)(
            self.state,
            modkey as *const _ as *const i8,
            xkb_state_component::XKB_STATE_MODS_EFFECTIVE,
        ).into()
    }

    pub unsafe fn get_modifiers(&mut self) -> ModifiersState {
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

    unsafe fn get_utf8_direct(&mut self, keycode: xkb_keycode_t) -> Option<String> {
        // This function returns the required size, and is so friendly that it specifies the
        // pattern of passing a NULL pointer to get the size without doing anything else.
        let required_size =
            (XKBCOMMON_HANDLE.xkb_state_key_get_utf8)(self.state, keycode, ptr::null_mut(), 0);

        if required_size == 0 {
            return None;
        }

        // Note that the returned size doesn't include the NULL byte.
        let buffer_size = (required_size + 1) as usize;
        let mut buffer: Vec<u8> = Vec::with_capacity(buffer_size);

        let bytes_written = (XKBCOMMON_HANDLE.xkb_state_key_get_utf8)(
            self.state,
            keycode,
            buffer.as_mut_ptr() as *mut i8,
            buffer_size,
        );

        // Check for truncation (which should never happen if we did the math right)
        assert!((bytes_written + 1) as usize == buffer_size);

        buffer.set_len(bytes_written as usize);

        // libxkbcommon always provides valid UTF8
        Some(String::from_utf8_unchecked(buffer))
    }

    pub unsafe fn get_utf8(
        &mut self,
        keycode: xkb_keycode_t,
        element_state: ElementState,
    ) -> Option<String> {
        if element_state == ElementState::Pressed {
            if self.is_composed() {
                self.compose.as_mut().unwrap().get_utf8()
            } else if self.is_composing() {
                None
            } else {
                self.get_utf8_direct(keycode)
            }
        } else {
            None
        }
    }

    pub unsafe fn update(
        &mut self,
        depressed_mods: xkb_mod_mask_t,
        latched_mods: xkb_mod_mask_t,
        locked_mods: xkb_mod_mask_t,
        depressed_layout: xkb_layout_index_t,
        latched_layout: xkb_layout_index_t,
        locked_layout: xkb_layout_index_t,
    ) {
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
