use std::{self, slice, str};
use std::boxed::Box;
use std::collections::VecDeque;
use std::os::raw::*;
use std::sync::Weak;

use cocoa::base::{class, id, nil};
use cocoa::appkit::NSWindow;
use cocoa::foundation::{NSPoint, NSRect, NSSize, NSString, NSUInteger};
//use core_foundation::string::UniChar;
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Protocol, Sel, BOOL};

use {ElementState, Event, KeyboardInput, WindowEvent, WindowId};
use platform::platform::events_loop::{DEVICE_ID, event_mods, Shared, to_virtual_key_code};
use platform::platform::input_client::*;
use platform::platform::util;
use platform::platform::window::{get_window_id, IdRef};

struct ViewState {
    window: id,
    shared: Weak<Shared>,
}

pub fn new_view(window: id, shared: Weak<Shared>) -> IdRef {
    let state = ViewState { window, shared };
    unsafe {
        // This is free'd in `dealloc`
        let state_ptr = Box::into_raw(Box::new(state)) as *mut c_void;
        let view: id = msg_send![VIEW_CLASS.0, alloc];
        IdRef::new(msg_send![view, initWithWinit:state_ptr])
    }
}

struct ViewClass(*const Class);
unsafe impl Send for ViewClass {}
unsafe impl Sync for ViewClass {}

lazy_static! {
    static ref VIEW_CLASS: ViewClass = unsafe {
        let superclass = Class::get("NSView").unwrap();
        let mut decl = ClassDecl::new("WinitView", superclass).unwrap();
        decl.add_method(sel!(dealloc), dealloc as extern fn(&Object, Sel));
        decl.add_method(
            sel!(initWithWinit:),
            init_with_winit as extern fn(&Object, Sel, *mut c_void) -> id,
        );
        decl.add_method(sel!(hasMarkedText), has_marked_text as extern fn(&Object, Sel) -> BOOL);
        decl.add_method(
            sel!(markedRange),
            marked_range as extern fn(&Object, Sel) -> NSRange,
        );
        decl.add_method(sel!(selectedRange), selected_range as extern fn(&Object, Sel) -> NSRange);
        decl.add_method(
            sel!(setMarkedText:selectedRange:replacementRange:),
            set_marked_text as extern fn(&mut Object, Sel, id, NSRange, NSRange),
        );
        decl.add_method(sel!(unmarkText), unmark_text as extern fn(&Object, Sel));
        decl.add_method(
            sel!(validAttributesForMarkedText),
            valid_attributes_for_marked_text as extern fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(attributedSubstringForProposedRange:actualRange:),
            attributed_substring_for_proposed_range
                as extern fn(&Object, Sel, NSRange, *mut c_void) -> id,
        );
        decl.add_method(
            sel!(insertText:replacementRange:),
            insert_text as extern fn(&Object, Sel, id, NSRange),
        );
        decl.add_method(
            sel!(characterIndexForPoint:),
            character_index_for_point as extern fn(&Object, Sel, NSPoint) -> NSUInteger,
        );
        decl.add_method(
            sel!(firstRectForCharacterRange:actualRange:),
            first_rect_for_character_range
                as extern fn(&Object, Sel, NSRange, *mut c_void) -> NSRect,
        );
        decl.add_method(
            sel!(doCommandBySelector:),
            do_command_by_selector as extern fn(&Object, Sel, Sel),
        );
        decl.add_method(sel!(keyDown:), key_down as extern fn(&Object, Sel, id));
        decl.add_method(sel!(insertTab:), insert_tab as extern fn(&Object, Sel, id));
        decl.add_method(sel!(insertBackTab:), insert_back_tab as extern fn(&Object, Sel, id));
        decl.add_ivar::<*mut c_void>("winitState");
        decl.add_ivar::<id>("trackingArea");
        decl.add_ivar::<id>("markedText");
        let protocol = Protocol::get("NSTextInputClient").unwrap();
        decl.add_protocol(&protocol);
        ViewClass(decl.register())
    };
}

extern fn dealloc(this: &Object, _sel: Sel) {
    println!("dealloc");
    unsafe {
        let state: *mut c_void = *this.get_ivar("winitState");
        let tracking_area: id = *this.get_ivar("trackingArea");
        let marked_text: id = *this.get_ivar("markedText");
        let _: () = msg_send![tracking_area, release];
        let _: () = msg_send![marked_text, release];
        Box::from_raw(state as *mut ViewState);
    }
}

extern fn init_with_winit(this: &Object, _sel: Sel, state: *mut c_void) -> id {
    println!("init_with_winit");
    unsafe {
        let this: id = msg_send![this, init];
        if this != nil {
            (*this).set_ivar("winitState", state);
            (*this).set_ivar("trackingArea", nil);
            let marked_text = <id as NSMutableAttributedString>::init(
                NSMutableAttributedString::alloc(nil),
            );
            (*this).set_ivar("markedText", marked_text);
        }
        this
    }
}

extern fn has_marked_text(this: &Object, _sel: Sel) -> BOOL {
    println!("has_marked_text");
    unsafe {
        let marked_text: id = *this.get_ivar("markedText");
        (marked_text.length() > 0) as i8
    }
}

extern fn marked_range(this: &Object, _sel: Sel) -> NSRange {
    println!("marked_range");
    unsafe {
        let marked_text: id = *this.get_ivar("markedText");
        let length = marked_text.length();
        if length > 0 {
            NSRange::new(0, length - 1)
        } else {
            EMPTY_RANGE
        }
    }
}

extern fn selected_range(_this: &Object, _sel: Sel) -> NSRange {
    println!("selected_range");
    EMPTY_RANGE
}

extern fn set_marked_text(
    this: &mut Object,
    _sel: Sel,
    string: id,
    _selected_range: NSRange,
    _replacement_range: NSRange,
) {
    println!("set_marked_text");
    unsafe {
        let marked_text_ref: &mut id = this.get_mut_ivar("markedText");
        let _: () = msg_send![(*marked_text_ref), release];
        let marked_text = NSMutableAttributedString::alloc(nil);
        let has_attr = msg_send![string, isKindOfClass:class("NSAttributedString")];
        if has_attr {
            marked_text.initWithAttributedString(string);
        } else {
            marked_text.initWithString(string);
        };
        *marked_text_ref = marked_text;
    }
}

extern fn unmark_text(this: &Object, _sel: Sel) {
    println!("unmark_text");
    unsafe {
        let marked_text: id = *this.get_ivar("markedText");
        let mutable_string = marked_text.mutableString();
        let _: () = msg_send![mutable_string, setString:""];
    }
}

extern fn valid_attributes_for_marked_text(_this: &Object, _sel: Sel) -> id {
    println!("valid_attributes_for_marked_text");
    unsafe { msg_send![class("NSArray"), array] }
}

extern fn attributed_substring_for_proposed_range(
    _this: &Object,
    _sel: Sel,
    _range: NSRange,
    _actual_range: *mut c_void, // *mut NSRange
) -> id {
    println!("attribute_substring_for_proposed_range");
    nil
}

extern fn character_index_for_point(_this: &Object, _sel: Sel, _point: NSPoint) -> NSUInteger {
    println!("character_index_for_point");
    0
}

extern fn first_rect_for_character_range(
    this: &Object,
    _sel: Sel,
    _range: NSRange,
    _actual_range: *mut c_void, // *mut NSRange
) -> NSRect {
    println!("first_rect_for_character_range");
    //const NSRect contentRect = [window->ns.view frame];
    unsafe {
        let state_ptr: *mut c_void = *this.get_ivar("winitState");
        let state = &mut *(state_ptr as *mut ViewState);
        let frame_rect = NSWindow::frame(state.window);
        let x = frame_rect.origin.x;
        let y = util::bottom_left_to_top_left(frame_rect);
        NSRect::new(
            NSPoint::new(x as _, y as _),
            NSSize::new(0.0, 0.0),
        )
    }
}

extern fn insert_text(this: &Object, _sel: Sel, string: id, _replacement_range: NSRange) {
    /*NSEvent* event = [NSApp currentEvent];
    const int mods = translateFlags([event modifierFlags]);
    const int plain = !(mods & GLFW_MOD_SUPER);*/
    println!("insert_text");

    unsafe {
        let state_ptr: *mut c_void = *this.get_ivar("winitState");
        let state = &mut *(state_ptr as *mut ViewState);

        let has_attr = msg_send![string, isKindOfClass:class("NSAttributedString")];
        let characters = if has_attr {
            // This is a *mut NSAttributedString
            msg_send![string, string]
        } else {
            // This is already a *mut NSString
            string
        };

        let slice = slice::from_raw_parts(
            characters.UTF8String() as *const c_uchar,
            characters.len(),
        );
        println!("{:?}", slice);
        let string = str::from_utf8_unchecked(slice);

        let mut events = VecDeque::with_capacity(characters.len());
        for character in string.chars() {
            /*let codepoint: UniChar = msg_send![characters, characterAtIndex:index];
            index += 1;
            if codepoint & 0xff00 == 0xf700 {
                continue;
            }*/
            events.push_back(Event::WindowEvent {
                window_id: WindowId(get_window_id(state.window)),
                event: WindowEvent::ReceivedCharacter(character),
            });
        }

        if let Some(shared) = state.shared.upgrade() {
            shared.pending_events
                .lock()
                .unwrap()
                .extend(events.into_iter());
        }
    }
}

extern fn do_command_by_selector(_this: &Object, _sel: Sel, _sel_arg: Sel) {
    println!("do_command_by_selector");
    /*unsafe {
        let _: () = msg_send![class("NSView"), doCommandBySelector:sel_arg];
    }*/
}

extern fn key_down(this: &Object, _sel: Sel, event: id) {
    unsafe {
        let state_ptr: *mut c_void = *this.get_ivar("winitState");
        let state = &mut *(state_ptr as *mut ViewState);

        let keycode: c_ushort = msg_send![event, keyCode];
        let virtual_keycode = to_virtual_key_code(keycode);
        let scancode = keycode as u32;
        let window_event = Event::WindowEvent {
            window_id: WindowId(get_window_id(state.window)),
            event: WindowEvent::KeyboardInput {
                device_id: DEVICE_ID,
                input: KeyboardInput {
                    state: ElementState::Pressed,
                    scancode,
                    virtual_keycode,
                    modifiers: event_mods(event),
                },
            },
        };

        if let Some(shared) = state.shared.upgrade() {
            shared.pending_events
                .lock()
                .unwrap()
                .push_back(window_event);
        }

        let array: id = msg_send![class("NSArray"), arrayWithObject:event];
        let (): _ = msg_send![this, interpretKeyEvents:array];
    }
}

extern fn insert_tab(this: &Object, _sel: Sel, _sender: id) {
    unsafe {
        let window: id = msg_send![this, window];
        let first_responder: id = msg_send![window, firstResponder];
        let this_ptr = this as *const _ as *mut _;
        if first_responder == this_ptr {
            let (): _ = msg_send![window, selectNextKeyView:this];
        }
    }
}

extern fn insert_back_tab(this: &Object, _sel: Sel, _sender: id) {
    unsafe {
        let window: id = msg_send![this, window];
        let first_responder: id = msg_send![window, firstResponder];
        let this_ptr = this as *const _ as *mut _;
        if first_responder == this_ptr {
            let (): _ = msg_send![window, selectPreviousKeyView:this];
        }
    }
}
