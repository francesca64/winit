use cocoa::base::{class, id, nil};
use cocoa::foundation::NSRect;
use core_graphics::display::CGDisplay;

// For consistency with other platforms, this will...
// 1. translate the bottom-left window corner into the top-left window corner
// 2. translate the coordinate from a bottom-left origin coordinate system to a top-left one
pub fn bottom_left_to_top_left(rect: NSRect) -> i32 {
    (CGDisplay::main().pixels_high() as f64 - (rect.origin.y + rect.size.height)) as _
}

#[allow(dead_code)]
pub unsafe fn open_emoji_picker() {
    let app: id = msg_send![class("NSApplication"), sharedApplication];
    let _: () = msg_send![app, orderFrontCharacterPalette:nil];
}