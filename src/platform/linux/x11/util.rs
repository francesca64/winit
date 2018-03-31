use std::mem;
use std::ptr;
use std::sync::Arc;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::os::raw::{c_char, c_double, c_int, c_long, c_short, c_uchar, c_uint, c_ulong};

use super::{ffi, XConnection, XError, WindowId};
use events::ModifiersState;

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
    event.format = 32;
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

#[derive(Debug)]
pub enum GetPropertyError {
    XError(XError),
    TypeMismatch(ffi::Atom),
    FormatMismatch(c_int),
    NothingAllocated,
}

pub unsafe fn get_property<T>(
    xconn: &Arc<XConnection>,
    window: c_ulong,
    property: ffi::Atom,
    property_type: ffi::Atom,
) -> Result<Vec<T>, GetPropertyError> {
    let mut data = Vec::new();

    let mut done = false;
    while !done {
        let mut actual_type: ffi::Atom = mem::uninitialized();
        let mut actual_format: c_int = mem::uninitialized();
        let mut byte_count: c_ulong = mem::uninitialized();
        let mut bytes_after: c_ulong = mem::uninitialized();
        let mut buf: *mut c_uchar = ptr::null_mut();
        (xconn.xlib.XGetWindowProperty)(
            xconn.display,
            window,
            property,
            (data.len() / 4) as c_long,
            1024,
            ffi::False,
            property_type,
            &mut actual_type,
            &mut actual_format,
            &mut byte_count,
            &mut bytes_after,
            &mut buf,
        );

        if let Err(e) = xconn.check_errors() {
            return Err(GetPropertyError::XError(e));
        }

        if actual_type != property_type {
            return Err(GetPropertyError::TypeMismatch(actual_type));
        }

        // Fun fact: actual_format ISN'T the size of the type; it's more like a really bad enum
        let format_mismatch = match actual_format as usize {
            8 => mem::size_of::<T>() != mem::size_of::<c_char>(),
            16 => mem::size_of::<T>() != mem::size_of::<c_short>(),
            32 => mem::size_of::<T>() != mem::size_of::<c_long>(),
            _ => true, // this won't actually be reached; the XError condition above is triggered
        };

        if format_mismatch {
            return Err(GetPropertyError::FormatMismatch(actual_format));
        }

        if !buf.is_null() {
            let mut buf =
                Vec::from_raw_parts(buf as *mut T, byte_count as usize, byte_count as usize);
            data.append(&mut buf);
        } else {
            return Err(GetPropertyError::NothingAllocated);
        }

        done = bytes_after == 0;
    }

    Ok(data)
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

// Important: all XIM calls need to happen from the same thread!
pub struct Ime {
    xconn: Arc<XConnection>,
    pub im: ffi::XIM,
    pub ic: ffi::XIC,
    ic_spot: ffi::XPoint,
    client_data: Box<ImeClientData>,
}

struct ImeClientData {
    pub xconn: Arc<XConnection>,
    pub window: ffi::Window,
    pub ime_map: *mut HashMap<WindowId, Ime>,
    pub ic_spot: ffi::XPoint,
    // Indicates whether or not the the input method was destroyed on the server end
    // (i.e. if ibus/etc. was terminated/restarted)
    pub destroyed: bool,
}

impl ImeClientData {
    pub fn new(
        xconn: Arc<XConnection>,
        window: ffi::Window,
        ime_map: *mut HashMap<WindowId, Ime>,
        ic_spot: Option<ffi::XPoint>,
    ) -> Self {
        ImeClientData {
            xconn,
            window,
            ime_map,
            ic_spot: ic_spot.unwrap_or_else(|| ffi::XPoint { x: 0, y: 0 }),
            destroyed: false,
        }
    }
}

unsafe fn xim_set_callback(
    xconn: &Arc<XConnection>,
    xim: ffi::XIM,
    field: *const c_char,
    callback: *mut ffi::XIMCallback,
) -> Result<(), XError> {
    (xconn.xlib.XSetIMValues)(
        xim,
        field,
        callback,
        ptr::null_mut::<()>(),
    );
    xconn.check_errors()
}

unsafe extern fn xim_instantiate_callback(
    _display: *mut ffi::Display,
    client_data: ffi::XPointer,
    _call_data: ffi::XPointer,
) {
    println!("INSTANTIATE=XIM");
    let client_data: *mut ImeClientData = client_data as _;
    if !client_data.is_null() {
        let xconn = &(*client_data).xconn;
        let window = (*client_data).window;
        (xconn.xlib.XUnregisterIMInstantiateCallback)(
            xconn.display,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            Some(xim_instantiate_callback),
            client_data as _,
        );
        let ime_map = (*client_data).ime_map;
        if let Some(old_ime) = (*ime_map).get_mut(&WindowId(window)) {
            println!("get");
            let new_ime = Ime::from_client_data(&mut *client_data)
                .expect("Failed to reinitialize IME");
            println!("new");
            let old = mem::replace(old_ime, new_ime);
            println!("replace");
            drop(old);
            println!("drop");
        }
    }
}

unsafe extern fn xim_destroy_callback(
    _xim: ffi::XIM,
    client_data: ffi::XPointer,
    // This field is unused
    _call_data: ffi::XPointer,
) {
    println!("DESTROYED=XIM");
    let client_data: *mut ImeClientData = client_data as _;
    if !client_data.is_null() {
        (*client_data).destroyed = true;
        let xconn = &(*client_data).xconn;
        (xconn.xlib.XRegisterIMInstantiateCallback)(
            xconn.display,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            Some(xim_instantiate_callback),
            client_data as _,
        );
    }
}

impl Ime {
    pub fn new(
        xconn: Arc<XConnection>,
        window: ffi::Window,
        ime_map: *mut HashMap<WindowId, Ime>,
        ic_spot: Option<ffi::XPoint>,
    ) -> Option<Self> {
        let mut client_data = Box::new(ImeClientData::new(
            Arc::clone(&xconn),
            window,
            ime_map,
            ic_spot,
        ));

        let im = unsafe {
            let im = (xconn.xlib.XOpenIM)(
                xconn.display,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            if im.is_null() {
                return None;
            }
            let client_data_ptr = Box::into_raw(client_data);
            let destroy_callback = ffi::XIMCallback {
                client_data: client_data_ptr as _,
                callback: Some(xim_destroy_callback),
            };
            client_data = Box::from_raw(client_data_ptr);
            xim_set_callback(
                &xconn,
                im,
                ffi::XNDestroyCallback_0.as_ptr() as *const _,
                Box::into_raw(Box::new(destroy_callback)),
            ).expect("Failed to set XNDestroyCallback on input method");
            im
        };

        let ic = unsafe {
            let ic = if let Some(ic_spot) = ic_spot {
                let preedit_attr = (xconn.xlib.XVaCreateNestedList)(
                    0,
                    ffi::XNSpotLocation_0.as_ptr() as *const _,
                    &ic_spot,
                    ptr::null_mut::<()>(),
                );
                (xconn.xlib.XCreateIC)(
                    im,
                    ffi::XNInputStyle_0.as_ptr() as *const _,
                    ffi::XIMPreeditNothing | ffi::XIMStatusNothing,
                    ffi::XNClientWindow_0.as_ptr() as *const _,
                    window,
                    ffi::XNPreeditAttributes_0.as_ptr() as *const _,
                    preedit_attr,
                    ptr::null_mut::<()>(),
                )
            } else {
                (xconn.xlib.XCreateIC)(
                    im,
                    ffi::XNInputStyle_0.as_ptr() as *const _,
                    ffi::XIMPreeditNothing | ffi::XIMStatusNothing,
                    ffi::XNClientWindow_0.as_ptr() as *const _,
                    window,
                    ptr::null_mut::<()>(),
                )
            };
            if ic.is_null() {
                return None;
            }
            (xconn.xlib.XSetICFocus)(ic);
            xconn.check_errors().expect("Failed to call XSetICFocus");
            ic
        };

        Some(Ime {
            xconn,
            im,
            ic,
            ic_spot: ic_spot.unwrap_or_else(|| ffi::XPoint { x: 0, y: 0 }),
            client_data,
        })
    }

    fn from_client_data(client_data: &ImeClientData) -> Option<Self> {
        Self::new(
            Arc::clone(&client_data.xconn),
            client_data.window,
            client_data.ime_map,
            Some(client_data.ic_spot),
        )
    }

    pub fn is_destroyed(&self) -> bool {
        self.client_data.destroyed
    }

    pub fn focus(&self) -> Result<(), XError> {
        if self.is_destroyed() {
            return Ok(());
        }
        unsafe {
            (self.xconn.xlib.XSetICFocus)(self.ic);
        }
        self.xconn.check_errors()
    }

    pub fn unfocus(&self) -> Result<(), XError> {
        if self.is_destroyed() {
            return Ok(());
        }
        unsafe {
            (self.xconn.xlib.XUnsetICFocus)(self.ic);
        }
        self.xconn.check_errors()
    }

    pub fn send_xim_spot(&mut self, x: i16, y: i16) {
        if self.is_destroyed() {
            return;
        }

        let nspot = ffi::XPoint { x: x as _, y: y as _ };
        if self.ic_spot.x == x && self.ic_spot.y == y {
            return;
        }
        self.ic_spot = nspot;
        self.client_data.ic_spot = nspot;

        unsafe {
            let preedit_attr = (self.xconn.xlib.XVaCreateNestedList)(
                0,
                ffi::XNSpotLocation_0.as_ptr() as *const _,
                &nspot,
                ptr::null_mut::<()>(),
            );
            (self.xconn.xlib.XSetICValues)(
                self.ic,
                ffi::XNPreeditAttributes_0.as_ptr() as *const _,
                preedit_attr,
                ptr::null_mut::<()>(),
            );
            (self.xconn.xlib.XFree)(preedit_attr);
        }
    }
}

impl Drop for Ime {
    fn drop(&mut self) {
        if !self.is_destroyed() {
            unsafe {
                (self.xconn.xlib.XDestroyIC)(self.ic);
                (self.xconn.xlib.XCloseIM)(self.im);
            }
            self.xconn.check_errors().expect("Failed to close input method");
        }
    }
}
