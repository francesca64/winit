use std::{ops::Deref, os::raw::c_void, sync::{Mutex, Weak}};

use cocoa::{
    appkit::{CGFloat, NSApp, NSImage, NSWindow, NSWindowStyleMask},
    base::{id, nil},
    foundation::{
        NSAutoreleasePool, NSDictionary, NSPoint, NSRect, NSSize, NSString,
        NSUInteger,
    },
};
use core_graphics::display::CGDisplay;
use objc::runtime::{BOOL, Class, Object, Sel, YES};

pub use util::*;
use {dpi::LogicalSize, window::MouseCursor};
use platform_impl::platform::{dispatch::*, ffi, window::SharedState};

pub const EMPTY_RANGE: ffi::NSRange = ffi::NSRange {
    location: ffi::NSNotFound as NSUInteger,
    length: 0,
};

pub struct IdRef(id);

impl IdRef {
    pub fn new(i: id) -> IdRef {
        IdRef(i)
    }

    #[allow(dead_code)]
    pub fn retain(i: id) -> IdRef {
        if i != nil {
            let _: id = unsafe { msg_send![i, retain] };
        }
        IdRef(i)
    }

    pub fn non_nil(self) -> Option<IdRef> {
        if self.0 == nil { None } else { Some(self) }
    }
}

impl Drop for IdRef {
    fn drop(&mut self) {
        if self.0 != nil {
            unsafe {
                let autoreleasepool = NSAutoreleasePool::new(nil);
                let _ : () = msg_send![self.0, release];
                let _ : () = msg_send![autoreleasepool, release];
            };
        }
    }
}

impl Deref for IdRef {
    type Target = id;
    fn deref<'a>(&'a self) -> &'a id {
        &self.0
    }
}

impl Clone for IdRef {
    fn clone(&self) -> IdRef {
        if self.0 != nil {
            let _: id = unsafe { msg_send![self.0, retain] };
        }
        IdRef(self.0)
    }
}

// For consistency with other platforms, this will...
// 1. translate the bottom-left window corner into the top-left window corner
// 2. translate the coordinate from a bottom-left origin coordinate system to a top-left one
pub fn bottom_left_to_top_left(rect: NSRect) -> f64 {
    CGDisplay::main().pixels_high() as f64 - (rect.origin.y + rect.size.height)
}

unsafe fn set_style_mask(nswindow: id, nsview: id, mask: NSWindowStyleMask) {
    nswindow.setStyleMask_(mask);
    // If we don't do this, key handling will break
    // (at least until the window is clicked again/etc.)
    nswindow.makeFirstResponder_(nsview);
}

struct SetStyleMaskData {
    nswindow: id,
    nsview: id,
    mask: NSWindowStyleMask,
}
impl SetStyleMaskData {
    fn new_ptr(
        nswindow: id,
        nsview: id,
        mask: NSWindowStyleMask,
    ) -> *mut Self {
        Box::into_raw(Box::new(SetStyleMaskData { nswindow, nsview, mask }))
    }
}
extern fn set_style_mask_callback(context: *mut c_void) {
    unsafe {
        let context_ptr = context as *mut SetStyleMaskData;
        {
            let context = &*context_ptr;
            set_style_mask(context.nswindow, context.nsview, context.mask);
        }
        Box::from_raw(context_ptr);
    }
}
// Always use this function instead of trying to modify `styleMask` directly!
// `setStyleMask:` isn't thread-safe, so we have to use Grand Central Dispatch.
// Otherwise, this would vomit out errors about not being on the main thread
// and fail to do anything.
pub unsafe fn set_style_mask_async(nswindow: id, nsview: id, mask: NSWindowStyleMask) {
    let context = SetStyleMaskData::new_ptr(nswindow, nsview, mask);
    dispatch_async_f(
        dispatch_get_main_queue(),
        context as *mut _,
        Some(set_style_mask_callback),
    );
}
pub unsafe fn set_style_mask_sync(nswindow: id, nsview: id, mask: NSWindowStyleMask) {
    let context = SetStyleMaskData::new_ptr(nswindow, nsview, mask);
    dispatch_sync_f(
        dispatch_get_main_queue(),
        context as *mut _,
        Some(set_style_mask_callback),
    );
}

struct SetContentSizeData {
    nswindow: id,
    size: LogicalSize,
}
impl SetContentSizeData {
    fn new_ptr(
        nswindow: id,
        size: LogicalSize,
    ) -> *mut Self {
        Box::into_raw(Box::new(SetContentSizeData { nswindow, size }))
    }
}
extern fn set_content_size_callback(context: *mut c_void) {
    unsafe {
        let context_ptr = context as *mut SetContentSizeData;
        {
            let context = &*context_ptr;
            NSWindow::setContentSize_(
                context.nswindow,
                NSSize::new(
                    context.size.width as CGFloat,
                    context.size.height as CGFloat,
                ),
            );
        }
        Box::from_raw(context_ptr);
    }
}
// `setContentSize:` isn't thread-safe either, though it doesn't log any errors
// and just fails silently. Anyway, GCD to the rescue!
pub unsafe fn set_content_size_async(nswindow: id, size: LogicalSize) {
    let context = SetContentSizeData::new_ptr(nswindow, size);
    dispatch_async_f(
        dispatch_get_main_queue(),
        context as *mut _,
        Some(set_content_size_callback),
    );
}

struct SetFrameTopLeftPointData {
    nswindow: id,
    point: NSPoint,
}
impl SetFrameTopLeftPointData {
    fn new_ptr(
        nswindow: id,
        point: NSPoint,
    ) -> *mut Self {
        Box::into_raw(Box::new(SetFrameTopLeftPointData { nswindow, point }))
    }
}
extern fn set_frame_top_left_point_callback(context: *mut c_void) {
    unsafe {
        let context_ptr = context as *mut SetFrameTopLeftPointData;
        {
            let context = &*context_ptr;
            NSWindow::setFrameTopLeftPoint_(context.nswindow, context.point);
        }
        Box::from_raw(context_ptr);
    }
}
// `setFrameTopLeftPoint:` isn't thread-safe, but fortunately has the courtesy
// to log errors.
pub unsafe fn set_frame_top_left_point_async(nswindow: id, point: NSPoint) {
    let context = SetFrameTopLeftPointData::new_ptr(nswindow, point);
    dispatch_async_f(
        dispatch_get_main_queue(),
        context as *mut _,
        Some(set_frame_top_left_point_callback),
    );
}

struct SetLevelData {
    nswindow: id,
    level: ffi::NSWindowLevel,
}
impl SetLevelData {
    fn new_ptr(
        nswindow: id,
        level: ffi::NSWindowLevel,
    ) -> *mut Self {
        Box::into_raw(Box::new(SetLevelData { nswindow, level }))
    }
}
extern fn set_level_callback(context: *mut c_void) {
    unsafe {
        let context_ptr = context as *mut SetLevelData;
        {
            let context = &*context_ptr;
            context.nswindow.setLevel_(context.level as _);
        }
        Box::from_raw(context_ptr);
    }
}
// `setFrameTopLeftPoint:` isn't thread-safe, and fails silently.
pub unsafe fn set_level_async(nswindow: id, level: ffi::NSWindowLevel) {
    let context = SetLevelData::new_ptr(nswindow, level);
    dispatch_async_f(
        dispatch_get_main_queue(),
        context as *mut _,
        Some(set_level_callback),
    );
}

struct ToggleFullScreenData {
    nswindow: id,
    nsview: id,
    not_fullscreen: bool,
    shared_state: Weak<Mutex<SharedState>>,
}
impl ToggleFullScreenData {
    fn new_ptr(
        nswindow: id,
        nsview: id,
        not_fullscreen: bool,
        shared_state: Weak<Mutex<SharedState>>,
    ) -> *mut Self {
        Box::into_raw(Box::new(ToggleFullScreenData {
            nswindow,
            nsview,
            not_fullscreen,
            shared_state,
        }))
    }
}
extern fn toggle_full_screen_callback(context: *mut c_void) {
    unsafe {
        let context_ptr = context as *mut ToggleFullScreenData;
        {
            let context = &*context_ptr;

            // `toggleFullScreen` doesn't work if the `StyleMask` is none, so we
            // set a normal style temporarily. The previous state will be
            // restored in `WindowDelegate::window_did_exit_fullscreen`.
            if context.not_fullscreen {
                let curr_mask = context.nswindow.styleMask();
                let required = NSWindowStyleMask::NSTitledWindowMask
                    | NSWindowStyleMask::NSResizableWindowMask;
                if !curr_mask.contains(required) {
                    set_style_mask(context.nswindow, context.nsview, required);
                    if let Some(shared_state) = context.shared_state.upgrade() {
                        trace!("Locked shared state in `toggle_full_screen_callback`");
                        let mut shared_state_lock = shared_state.lock().unwrap();
                        (*shared_state_lock).saved_style = Some(curr_mask);
                        trace!("Unlocked shared state in `toggle_full_screen_callback`");
                    }
                }
            }

            context.nswindow.toggleFullScreen_(nil);
        }
        Box::from_raw(context_ptr);
    }
}
// `toggleFullScreen` is thread-safe, but our additional logic to account for
// window styles isn't.
pub unsafe fn toggle_full_screen_async(
    nswindow: id,
    nsview: id,
    not_fullscreen: bool,
    shared_state: Weak<Mutex<SharedState>>,
) {
    let context = ToggleFullScreenData::new_ptr(
        nswindow,
        nsview,
        not_fullscreen,
        shared_state,
    );
    dispatch_async_f(
        dispatch_get_main_queue(),
        context as *mut _,
        Some(toggle_full_screen_callback),
    );
}

struct OrderOutData {
    nswindow: id,
}
impl OrderOutData {
    fn new_ptr(nswindow: id) -> *mut Self {
        Box::into_raw(Box::new(OrderOutData { nswindow }))
    }
}
extern fn order_out_callback(context: *mut c_void) {
    unsafe {
        let context_ptr = context as *mut OrderOutData;
        {
            let context = &*context_ptr;
            context.nswindow.orderOut_(nil);
        }
        Box::from_raw(context_ptr);
    }
}
// `orderOut:` isn't thread-safe. Calling it from another thread actually works,
// but with an odd delay.
pub unsafe fn order_out_async(nswindow: id) {
    let context = OrderOutData::new_ptr(nswindow);
    dispatch_async_f(
        dispatch_get_main_queue(),
        context as *mut _,
        Some(order_out_callback),
    );
}

struct MakeKeyAndOrderFrontData {
    nswindow: id,
}
impl MakeKeyAndOrderFrontData {
    fn new_ptr(nswindow: id) -> *mut Self {
        Box::into_raw(Box::new(MakeKeyAndOrderFrontData { nswindow }))
    }
}
extern fn make_key_and_order_front_callback(context: *mut c_void) {
    unsafe {
        let context_ptr = context as *mut MakeKeyAndOrderFrontData;
        {
            let context = &*context_ptr;
            context.nswindow.makeKeyAndOrderFront_(nil);
        }
        Box::from_raw(context_ptr);
    }
}
// `makeKeyAndOrderFront::` isn't thread-safe. Calling it from another thread
// actually works, but with an odd delay.
pub unsafe fn make_key_and_order_front_async(nswindow: id) {
    let context = MakeKeyAndOrderFrontData::new_ptr(nswindow);
    dispatch_async_f(
        dispatch_get_main_queue(),
        context as *mut _,
        Some(make_key_and_order_front_callback),
    );
}

struct CloseData {
    nswindow: id,
}
impl CloseData {
    fn new_ptr(nswindow: id) -> *mut Self {
        Box::into_raw(Box::new(CloseData { nswindow }))
    }
}
extern fn close_callback(context: *mut c_void) {
    unsafe {
        let context_ptr = context as *mut CloseData;
        {
            let context = &*context_ptr;
            let pool = NSAutoreleasePool::new(nil);
            context.nswindow.close();
            pool.drain();
        }
        Box::from_raw(context_ptr);
    }
}
// `makeKeyAndOrderFront::` isn't thread-safe. Calling it from another thread
// actually works, but with an odd delay.
pub unsafe fn close_async(nswindow: id) {
    let context = CloseData::new_ptr(nswindow);
    dispatch_async_f(
        dispatch_get_main_queue(),
        context as *mut _,
        Some(close_callback),
    );
}

pub unsafe fn superclass<'a>(this: &'a Object) -> &'a Class {
    let superclass: id = msg_send![this, superclass];
    &*(superclass as *const _)
}

pub unsafe fn create_input_context(view: id) -> IdRef {
    let input_context: id = msg_send![class!(NSTextInputContext), alloc];
    let input_context: id = msg_send![input_context, initWithClient:view];
    IdRef::new(input_context)
}

pub enum CursorType {
    Native(&'static str),
    Undocumented(&'static str),
    WebKit(&'static str),
}

impl From<MouseCursor> for CursorType {
    fn from(cursor: MouseCursor) -> Self {
        match cursor {
            MouseCursor::Arrow | MouseCursor::Default => CursorType::Native("arrowCursor"),
            MouseCursor::Hand => CursorType::Native("pointingHandCursor"),
            MouseCursor::Grabbing | MouseCursor::Grab => CursorType::Native("closedHandCursor"),
            MouseCursor::Text => CursorType::Native("IBeamCursor"),
            MouseCursor::VerticalText => CursorType::Native("IBeamCursorForVerticalLayout"),
            MouseCursor::Copy => CursorType::Native("dragCopyCursor"),
            MouseCursor::Alias => CursorType::Native("dragLinkCursor"),
            MouseCursor::NotAllowed | MouseCursor::NoDrop => CursorType::Native("operationNotAllowedCursor"),
            MouseCursor::ContextMenu => CursorType::Native("contextualMenuCursor"),
            MouseCursor::Crosshair => CursorType::Native("crosshairCursor"),
            MouseCursor::EResize => CursorType::Native("resizeRightCursor"),
            MouseCursor::NResize => CursorType::Native("resizeUpCursor"),
            MouseCursor::WResize => CursorType::Native("resizeLeftCursor"),
            MouseCursor::SResize => CursorType::Native("resizeDownCursor"),
            MouseCursor::EwResize | MouseCursor::ColResize => CursorType::Native("resizeLeftRightCursor"),
            MouseCursor::NsResize | MouseCursor::RowResize => CursorType::Native("resizeUpDownCursor"),

            // Undocumented cursors: https://stackoverflow.com/a/46635398/5435443
            MouseCursor::Help => CursorType::Undocumented("_helpCursor"),
            MouseCursor::ZoomIn => CursorType::Undocumented("_zoomInCursor"),
            MouseCursor::ZoomOut => CursorType::Undocumented("_zoomOutCursor"),
            MouseCursor::NeResize => CursorType::Undocumented("_windowResizeNorthEastCursor"),
            MouseCursor::NwResize => CursorType::Undocumented("_windowResizeNorthWestCursor"),
            MouseCursor::SeResize => CursorType::Undocumented("_windowResizeSouthEastCursor"),
            MouseCursor::SwResize => CursorType::Undocumented("_windowResizeSouthWestCursor"),
            MouseCursor::NeswResize => CursorType::Undocumented("_windowResizeNorthEastSouthWestCursor"),
            MouseCursor::NwseResize => CursorType::Undocumented("_windowResizeNorthWestSouthEastCursor"),

            // While these are available, the former just loads a white arrow,
            // and the latter loads an ugly deflated beachball!
            // MouseCursor::Move => CursorType::Undocumented("_moveCursor"),
            // MouseCursor::Wait => CursorType::Undocumented("_waitCursor"),

            // An even more undocumented cursor...
            // https://bugs.eclipse.org/bugs/show_bug.cgi?id=522349
            // This is the wrong semantics for `Wait`, but it's the same as
            // what's used in Safari and Chrome.
            MouseCursor::Wait | MouseCursor::Progress => CursorType::Undocumented("busyButClickableCursor"),

            // For the rest, we can just snatch the cursors from WebKit...
            // They fit the style of the native cursors, and will seem
            // completely standard to macOS users.
            // https://stackoverflow.com/a/21786835/5435443
            MouseCursor::Move | MouseCursor::AllScroll => CursorType::WebKit("move"),
            MouseCursor::Cell => CursorType::WebKit("cell"),
        }
    }
}

impl CursorType {
    pub unsafe fn load(self) -> id {
        match self {
            CursorType::Native(cursor_name) => {
                let sel = Sel::register(cursor_name);
                msg_send![class!(NSCursor), performSelector:sel]
            },
            CursorType::Undocumented(cursor_name) => {
                let class = class!(NSCursor);
                let sel = Sel::register(cursor_name);
                let sel = if msg_send![class, respondsToSelector:sel] {
                    sel
                } else {
                    warn!("Cursor `{}` appears to be invalid", cursor_name);
                    sel!(arrowCursor)
                };
                msg_send![class, performSelector:sel]
            },
            CursorType::WebKit(cursor_name) => load_webkit_cursor(cursor_name),
        }
    }
}

// Note that loading `busybutclickable` with this code won't animate the frames;
// instead you'll just get them all in a column.
pub unsafe fn load_webkit_cursor(cursor_name: &str) -> id {
    static CURSOR_ROOT: &'static str = "/System/Library/Frameworks/ApplicationServices.framework/Versions/A/Frameworks/HIServices.framework/Versions/A/Resources/cursors";
    let cursor_root = NSString::alloc(nil).init_str(CURSOR_ROOT);
    let cursor_name = NSString::alloc(nil).init_str(cursor_name);
    let cursor_pdf = NSString::alloc(nil).init_str("cursor.pdf");
    let cursor_plist = NSString::alloc(nil).init_str("info.plist");
    let key_x = NSString::alloc(nil).init_str("hotx");
    let key_y = NSString::alloc(nil).init_str("hoty");

    let cursor_path: id = msg_send![cursor_root,
        stringByAppendingPathComponent:cursor_name
    ];
    let pdf_path: id = msg_send![cursor_path,
        stringByAppendingPathComponent:cursor_pdf
    ];
    let info_path: id = msg_send![cursor_path,
        stringByAppendingPathComponent:cursor_plist
    ];

    let image = NSImage::alloc(nil).initByReferencingFile_(pdf_path);
    let info = NSDictionary::dictionaryWithContentsOfFile_(
        nil,
        info_path,
    );
    let x = info.valueForKey_(key_x);
    let y = info.valueForKey_(key_y);
    let point = NSPoint::new(
        msg_send![x, doubleValue],
        msg_send![y, doubleValue],
    );
    let cursor: id = msg_send![class!(NSCursor), alloc];
    msg_send![cursor,
        initWithImage:image
        hotSpot:point
    ]
}

#[allow(dead_code)]
pub unsafe fn open_emoji_picker() {
    let _: () = msg_send![NSApp(), orderFrontCharacterPalette:nil];
}

pub extern fn yes(_: &Object, _: Sel) -> BOOL {
    YES
}
