use std::{ops::Deref, sync::{Arc, Mutex, Weak}};

use cocoa::{
    appkit::NSWindowStyleMask,
    base::{id, nil},
    foundation::{NSAutoreleasePool, NSRect, NSUInteger},
};
use core_graphics::display::CGDisplay;
use objc::runtime::{BOOL, Object, Sel, YES};

use platform_impl::platform::ffi;

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

pub trait Access<T> {
    fn access<F: FnOnce(&mut T) -> O, O>(&self, callback: F) -> Option<O>;
}

impl<T> Access<T> for Arc<Mutex<T>> {
    fn access<F: FnOnce(&mut T) -> O, O>(&self, callback: F) -> Option<O> {
        self.lock()
            .ok()
            .map(|ref mut mutex_guard| callback(mutex_guard))
    }
}

impl<T> Access<T> for Weak<Mutex<T>> {
    fn access<F: FnOnce(&mut T) -> O, O>(&self, callback: F) -> Option<O> {
        self.upgrade()
            .and_then(|arc| arc.access(callback))
    }
}

// For consistency with other platforms, this will...
// 1. translate the bottom-left window corner into the top-left window corner
// 2. translate the coordinate from a bottom-left origin coordinate system to a top-left one
pub fn bottom_left_to_top_left(rect: NSRect) -> f64 {
    CGDisplay::main().pixels_high() as f64 - (rect.origin.y + rect.size.height)
}

pub unsafe fn set_style_mask(window: id, view: id, mask: NSWindowStyleMask) {
    use cocoa::appkit::NSWindow;
    window.setStyleMask_(mask);
    // If we don't do this, key handling will break. Therefore, never call `setStyleMask` directly!
    window.makeFirstResponder_(view);
}

pub unsafe fn create_input_context(view: id) -> IdRef {
    let input_context: id = msg_send![class!(NSTextInputContext), alloc];
    let input_context: id = msg_send![input_context, initWithClient:view];
    IdRef::new(input_context)
}

#[allow(dead_code)]
pub unsafe fn open_emoji_picker() {
    let app: id = msg_send![class!(NSApplication), sharedApplication];
    let _: () = msg_send![app, orderFrontCharacterPalette:nil];
}

pub extern fn yes(_: &Object, _: Sel) -> BOOL {
    YES
}
