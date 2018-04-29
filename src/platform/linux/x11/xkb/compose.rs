use std::env;
use std::ptr;
use std::ffi::CString;
use std::os::unix::ffi::OsStringExt;

use xkbcommon_dl::*;

lazy_static! {
    static ref LOCALE: CString = {
        let locale = env::var_os("LC_ALL")
            .or_else(|| env::var_os("LC_CTYPE"))
            .or_else(|| env::var_os("LANG"))
            .unwrap_or_else(|| "C".into());
        CString::new(locale.into_vec()).unwrap()
    };
}

#[derive(Debug)]
pub enum XkbComposeInitError {
    ComposeUnavailable,
    ComposeTableIsNull,
    ComposeStateIsNull,
}

#[derive(Debug)]
pub struct XkbCompose {
    compose_table: *mut xkb_compose_table,
    compose_state: *mut xkb_compose_state,
    pub compose_status: xkb_compose_status,
}

impl Drop for XkbCompose {
    fn drop(&mut self) {
        unsafe {
            (XKBCOMMON_COMPOSE_HANDLE.xkb_compose_state_unref)(self.compose_state);
            (XKBCOMMON_COMPOSE_HANDLE.xkb_compose_table_unref)(self.compose_table);
        }
    }
}

impl XkbCompose {
    pub unsafe fn new(context: *mut xkb_context) -> Result<Self, XkbComposeInitError> {
        if XKBCOMMON_COMPOSE_OPTION.is_none() {
            return Err(XkbComposeInitError::ComposeUnavailable);
        }

        let compose_table = (XKBCOMMON_COMPOSE_HANDLE.xkb_compose_table_new_from_locale)(
            context,
            LOCALE.as_ptr(),
            xkb_compose_compile_flags::XKB_COMPOSE_COMPILE_NO_FLAGS,
        );
        if compose_table.is_null() {
            return Err(XkbComposeInitError::ComposeTableIsNull);
        }

        let compose_state = (XKBCOMMON_COMPOSE_HANDLE.xkb_compose_state_new)(
            compose_table,
            xkb_compose_state_flags::XKB_COMPOSE_STATE_NO_FLAGS,
        );
        if compose_state.is_null() {
            (XKBCOMMON_COMPOSE_HANDLE.xkb_compose_table_unref)(compose_table);
            return Err(XkbComposeInitError::ComposeStateIsNull);
        }

        Ok(XkbCompose {
            compose_table,
            compose_state,
            compose_status: xkb_compose_status::XKB_COMPOSE_NOTHING,
        })
    }

    pub fn feed_keysym(&mut self, keysym: xkb_keysym_t) -> xkb_compose_feed_result {
        let result = unsafe {
            (XKBCOMMON_COMPOSE_HANDLE.xkb_compose_state_feed)(self.compose_state, keysym)
        };
        if result == xkb_compose_feed_result::XKB_COMPOSE_FEED_ACCEPTED {
            self.compose_status = self.get_status();
        }
        result
    }

    pub fn reset(&mut self) {
        unsafe {
            (XKBCOMMON_COMPOSE_HANDLE.xkb_compose_state_reset)(self.compose_state);
        }
        self.compose_status = xkb_compose_status::XKB_COMPOSE_NOTHING;
    }

    fn get_status(&self) -> xkb_compose_status {
        unsafe {
            (XKBCOMMON_COMPOSE_HANDLE.xkb_compose_state_get_status)(self.compose_state)
        }
    }

    pub fn get_keysym(&self) -> xkb_keysym_t {
        unsafe {
            (XKBCOMMON_COMPOSE_HANDLE.xkb_compose_state_get_one_sym)(self.compose_state)
        }
    }

    pub unsafe fn get_utf8(&mut self) -> Option<String> {
        assert!(self.compose_status == xkb_compose_status::XKB_COMPOSE_COMPOSED);

        // This function returns the required size, and is so friendly that it specifies the
        // pattern of passing a NULL pointer to get the size without doing anything else.
        let required_size = (XKBCOMMON_COMPOSE_HANDLE.xkb_compose_state_get_utf8)(
            self.compose_state,
            ptr::null_mut(),
            0,
        );

        if required_size == 0 {
            return None;
        }

        // Note that the returned size doesn't include the NULL byte.
        let buffer_size = (required_size + 1) as usize;
        let mut buffer: Vec<u8> = Vec::with_capacity(buffer_size);

        let bytes_written = (XKBCOMMON_COMPOSE_HANDLE.xkb_compose_state_get_utf8)(
            self.compose_state,
            buffer.as_mut_ptr() as *mut i8,
            buffer_size,
        );

        // Check for truncation (which should never happen if we did the math right)
        debug_assert_eq!((bytes_written + 1) as usize, buffer_size);

        buffer.set_len(bytes_written as usize);

        self.reset();

        // libxkbcommon always provides valid UTF8
        Some(String::from_utf8_unchecked(buffer))
    }
}
