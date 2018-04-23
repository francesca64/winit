use std::mem;
use std::ptr;
use std::str;
use std::sync::Arc;
use std::ops::{Deref, DerefMut};
use std::os::raw::{c_char, c_double, c_int, c_long, c_short, c_uchar, c_uint, c_ulong};

use super::{ffi, XConnection, XError};
use events::ModifiersState;

// This isn't actually the number of the bits in the format.
// X11 does a match on this value to determine which type to call sizeof on.
// Thus, we use 32 for c_long, since 32 maps to c_long which maps to 64.
// ...if that sounds confusing, then you know why this enum is here.
#[derive(Debug, Copy, Clone)]
pub enum Format {
    Char = 8,
    #[allow(dead_code)]
    Short = 16,
    Long = 32,
}

impl Format {
    pub fn from_format(format: usize) -> Option<Self> {
        match format {
            8 => Some(Format::Char),
            16 => Some(Format::Short),
            32 => Some(Format::Long),
            _ => None,
        }
    }

    pub fn is_same_size_as<T>(&self) -> bool {
        mem::size_of::<T>() == self.get_actual_size()
    }

    pub fn get_actual_size(&self) -> usize {
        match self {
            Format::Char => mem::size_of::<c_char>(),
            Format::Short => mem::size_of::<c_short>(),
            Format::Long => mem::size_of::<c_long>(),
        }
    }
}

pub struct XSmartPointer<'a, T> {
    xconn: &'a Arc<XConnection>,
    pub ptr: *mut T,
}

impl<'a, T> XSmartPointer<'a, T> {
    // You're responsible for only passing things to this that should be XFree'd.
    // Returns None if ptr is null.
    pub fn new(xconn: &'a Arc<XConnection>, ptr: *mut T) -> Option<Self> {
        if !ptr.is_null() {
            Some(XSmartPointer {
                xconn,
                ptr,
            })
        } else {
            None
        }
    }
}

impl<'a, T> Deref for XSmartPointer<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.ptr }
    }
}

impl<'a, T> DerefMut for XSmartPointer<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.ptr }
    }
}

impl<'a, T> Drop for XSmartPointer<'a, T> {
    fn drop(&mut self) {
        unsafe {
            (self.xconn.xlib.XFree)(self.ptr as *mut _);
        }
    }
}

pub unsafe fn get_atom(xconn: &Arc<XConnection>, name: &[u8]) -> Result<ffi::Atom, XError> {
    let atom_name: *const c_char = name.as_ptr() as _;
    let atom = (xconn.xlib.XInternAtom)(xconn.display, atom_name, ffi::False);
    xconn.check_errors().map(|_| atom)
}

pub unsafe fn send_client_msg(
    xconn: &Arc<XConnection>,
    window: c_ulong,        // the window this is "about"; not necessarily this window
    target_window: c_ulong, // the window we're sending to
    message_type: ffi::Atom,
    event_mask: Option<c_long>,
    data: (c_long, c_long, c_long, c_long, c_long),
) -> Result<(), XError> {
    let mut event: ffi::XClientMessageEvent = mem::uninitialized();
    event.type_ = ffi::ClientMessage;
    event.display = xconn.display;
    event.window = window;
    event.message_type = message_type;
    event.format = Format::Long as c_int;
    event.data = ffi::ClientMessageData::new();
    event.data.set_long(0, data.0);
    event.data.set_long(1, data.1);
    event.data.set_long(2, data.2);
    event.data.set_long(3, data.3);
    event.data.set_long(4, data.4);

    let event_mask = event_mask.unwrap_or(ffi::NoEventMask);

    (xconn.xlib.XSendEvent)(
        xconn.display,
        target_window,
        ffi::False,
        event_mask,
        &mut event.into(),
    );

    xconn.check_errors().map(|_| ())
}

#[derive(Debug, Clone)]
pub enum GetPropertyError {
    XError(XError),
    TypeMismatch(ffi::Atom),
    FormatMismatch(c_int),
    NothingAllocated,
}

impl GetPropertyError {
    pub fn is_actual_property_type(&self, t: ffi::Atom) -> bool {
        if let GetPropertyError::TypeMismatch(actual_type) = *self {
            actual_type == t
        } else {
            false
        }
    }
}

// Number of 32-bit chunks to retrieve per interation of get_property's inner loop.
// To test if get_property works correctly, set this to 1.
const PROPERTY_BUFFER_SIZE: c_long = 1024; // 4K of RAM ought to be enough for anyone!

pub unsafe fn get_property<T>(
    xconn: &Arc<XConnection>,
    window: c_ulong,
    property: ffi::Atom,
    property_type: ffi::Atom,
) -> Result<Vec<T>, GetPropertyError> {
    let mut data = Vec::new();
    let mut offset = 0;

    let mut done = false;
    while !done {
        let mut actual_type: ffi::Atom = mem::uninitialized();
        let mut actual_format: c_int = mem::uninitialized();
        let mut quantity_returned: c_ulong = mem::uninitialized();
        let mut bytes_after: c_ulong = mem::uninitialized();
        let mut buf: *mut c_uchar = ptr::null_mut();
        (xconn.xlib.XGetWindowProperty)(
            xconn.display,
            window,
            property,
            // This offset is in terms of 32-bit chunks.
            offset,
            // This is the quanity of 32-bit chunks to receive at once.
            PROPERTY_BUFFER_SIZE,
            ffi::False,
            property_type,
            &mut actual_type,
            &mut actual_format,
            // This is the quantity of items we retrieved in our format, NOT of 32-bit chunks!
            &mut quantity_returned,
            // ...and this is a quantity of bytes. So, this function deals in 3 different units.
            &mut bytes_after,
            &mut buf,
        );

        println!(
            "GET_PROPERTY fmt:{:02} len:{:02} off:{:02} out:{:02}",
            mem::size_of::<T>() * 8,
            data.len(),
            offset,
            quantity_returned,
        );

        if let Err(e) = xconn.check_errors() {
            return Err(GetPropertyError::XError(e));
        }

        if actual_type != property_type {
            return Err(GetPropertyError::TypeMismatch(actual_type));
        }

        let format_mismatch = Format::from_format(actual_format as _)
            .map(|actual_format| !actual_format.is_same_size_as::<T>())
            // this won't actually be reached; the XError condition above is triggered
            .unwrap_or(true);

        if format_mismatch {
            return Err(GetPropertyError::FormatMismatch(actual_format));
        }

        if !buf.is_null() {
            offset += PROPERTY_BUFFER_SIZE;
            let mut buf = Vec::from_raw_parts(
                buf as *mut T,
                quantity_returned as usize,
                quantity_returned as usize,
            );
            data.append(&mut buf);
        } else {
            return Err(GetPropertyError::NothingAllocated);
        }

        done = bytes_after == 0;
    }

    Ok(data)
}

#[derive(Debug)]
pub enum PropMode {
    Replace = ffi::PropModeReplace as isize,
    #[allow(dead_code)]
    Prepend = ffi::PropModePrepend as isize,
    #[allow(dead_code)]
    Append = ffi::PropModeAppend as isize,
}

#[derive(Debug, Clone)]
pub enum ChangePropertyError {
    XError(XError),
    FormatError {
        format_used: Format,
        size_passed: usize,
        size_expected: usize,
    },
}

pub unsafe fn change_property<T>(
    xconn: &Arc<XConnection>,
    window: c_ulong,
    property: ffi::Atom,
    property_type: ffi::Atom,
    format: Format,
    mode: PropMode,
    new_value: &[T],
) -> Result<(), ChangePropertyError> {
    if !format.is_same_size_as::<T>() {
        return Err(ChangePropertyError::FormatError {
            format_used: format,
            size_passed: mem::size_of::<T>() * 8,
            size_expected: format.get_actual_size() * 8,
        });
    }

    (xconn.xlib.XChangeProperty)(
        xconn.display,
        window,
        property,
        property_type,
        format as c_int,
        mode as c_int,
        new_value.as_ptr() as *const c_uchar,
        new_value.len() as c_int,
    );

    if let Err(e) = xconn.check_errors() {
        Err(ChangePropertyError::XError(e))
    } else {
        Ok(())
    }
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

    str::from_utf8(&buffer[..count as usize]).unwrap_or("").to_string()
}

#[derive(Debug)]
pub struct FrameExtents {
    pub left: c_ulong,
    pub right: c_ulong,
    pub top: c_ulong,
    pub bottom: c_ulong,
}

impl FrameExtents {
    pub fn new(left: c_ulong, right: c_ulong, top: c_ulong, bottom: c_ulong) -> Self {
        FrameExtents { left, right, top, bottom }
    }

    pub fn from_border(border: c_ulong) -> Self {
        Self::new(border, border, border, border)
    }
}

#[derive(Debug)]
pub struct WindowGeometry {
    pub x: c_int,
    pub y: c_int,
    pub width: c_uint,
    pub height: c_uint,
    pub frame: FrameExtents,
}

impl WindowGeometry {
    pub fn get_position(&self) -> (i32, i32) {
        (self.x as _, self.y as _)
    }

    pub fn get_inner_position(&self) -> (i32, i32) {
        (
            self.x.saturating_add(self.frame.left as c_int) as _,
            self.y.saturating_add(self.frame.top as c_int) as _,
        )
    }

    pub fn get_inner_size(&self) -> (u32, u32) {
        (self.width as _, self.height as _)
    }

    pub fn get_outer_size(&self) -> (u32, u32) {
        (
            self.width.saturating_add(
                self.frame.left.saturating_add(self.frame.right) as c_uint
            ) as _,
            self.height.saturating_add(
                self.frame.top.saturating_add(self.frame.bottom) as c_uint
            ) as _,
        )
    }
}
