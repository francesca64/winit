use std::mem;

use super::*;
use events::ModifiersState;

pub unsafe fn select_xinput_events(
    xconn: &Arc<XConnection>,
    window: c_ulong,
    device_id: c_int,
    mask: i32,
) -> Flusher {
    let mut event_mask = ffi::XIEventMask {
        deviceid: device_id,
        mask: &mask as *const _ as *mut c_uchar,
        mask_len: mem::size_of_val(&mask) as c_int,
    };
    (xconn.xinput2.XISelectEvents)(
        xconn.display,
        window,
        &mut event_mask as *mut ffi::XIEventMask,
        1, // number of masks to read from pointer above
    );
    Flusher::new(xconn)
}

impl From<ffi::XIModifierState> for ModifiersState {
    fn from(mods: ffi::XIModifierState) -> Self {
        let state = mods.effective as c_uint;
        ModifiersState {
            alt: state & ffi::Mod1Mask != 0,
            shift: state & ffi::ShiftMask != 0,
            ctrl: state & ffi::ControlMask != 0,
            logo: state & ffi::Mod4Mask != 0,
        }
    }
}

#[derive(Debug)]
pub struct PointerState {
    #[allow(dead_code)]
    root: ffi::Window,
    #[allow(dead_code)]
    child: ffi::Window,
    #[allow(dead_code)]
    root_x: c_double,
    #[allow(dead_code)]
    root_y: c_double,
    #[allow(dead_code)]
    win_x: c_double,
    #[allow(dead_code)]
    win_y: c_double,
    #[allow(dead_code)]
    buttons: ffi::XIButtonState,
    modifiers: ffi::XIModifierState,
    #[allow(dead_code)]
    group: ffi::XIGroupState,
    #[allow(dead_code)]
    relative_to_window: bool,
}

impl PointerState {
    pub fn get_modifier_state(&self) -> ModifiersState {
        self.modifiers.into()
    }
}

pub unsafe fn query_pointer(
    xconn: &Arc<XConnection>,
    window: ffi::Window,
    device_id: c_int,
) -> Result<PointerState, XError> {
    let mut root_return = mem::uninitialized();
    let mut child_return = mem::uninitialized();
    let mut root_x_return = mem::uninitialized();
    let mut root_y_return = mem::uninitialized();
    let mut win_x_return = mem::uninitialized();
    let mut win_y_return = mem::uninitialized();
    let mut buttons_return = mem::uninitialized();
    let mut modifiers_return = mem::uninitialized();
    let mut group_return = mem::uninitialized();

    let relative_to_window = (xconn.xinput2.XIQueryPointer)(
        xconn.display,
        device_id,
        window,
        &mut root_return,
        &mut child_return,
        &mut root_x_return,
        &mut root_y_return,
        &mut win_x_return,
        &mut win_y_return,
        &mut buttons_return,
        &mut modifiers_return,
        &mut group_return,
    ) == ffi::True;

    xconn.check_errors()?;

    Ok(PointerState {
        root: root_return,
        child: child_return,
        root_x: root_x_return,
        root_y: root_y_return,
        win_x: win_x_return,
        win_y: win_y_return,
        buttons: buttons_return,
        modifiers: modifiers_return,
        group: group_return,
        relative_to_window,
    })
}

unsafe fn lookup_utf8_inner(
    xconn: &Arc<XConnection>,
    ic: ffi::XIC,
    key_event: &mut ffi::XKeyEvent,
    buffer: &mut [u8],
) -> (ffi::KeySym, ffi::Status, c_int) {
    let mut keysym: ffi::KeySym = 0;
    let mut status: ffi::Status = 0;
    let count = (xconn.xlib.Xutf8LookupString)(
        ic,
        key_event,
        buffer.as_mut_ptr() as *mut c_char,
        buffer.len() as c_int,
        &mut keysym,
        &mut status,
    );
    (keysym, status, count)
}

pub unsafe fn lookup_utf8(
    xconn: &Arc<XConnection>,
    ic: ffi::XIC,
    key_event: &mut ffi::XKeyEvent,
) -> String {
    const INIT_BUFF_SIZE: usize = 16;

    // Buffer allocated on heap instead of stack, due to the possible reallocation
    let mut buffer: Vec<u8> = vec![mem::uninitialized(); INIT_BUFF_SIZE];
    let (_, status, mut count) = lookup_utf8_inner(
        xconn,
        ic,
        key_event,
        &mut buffer,
    );

    // Buffer overflowed, dynamically reallocate
    if status == ffi::XBufferOverflow {
        buffer = vec![mem::uninitialized(); count as usize];
        let (_, _, new_count) = lookup_utf8_inner(
            xconn,
            ic,
            key_event,
            &mut buffer,
        );
        count = new_count;
    }

    str::from_utf8(&buffer[..count as usize])
        .unwrap_or("")
        .to_string()
}
