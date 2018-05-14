use std::{slice, str};
use std::boxed::Box;
use std::collections::VecDeque;
use std::os::raw::*;
use std::sync::Weak;

use cocoa::base::{class, id, nil};
use cocoa::appkit::NSWindow;
use cocoa::foundation::{NSPoint, NSRect, NSSize, NSString, NSUInteger};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Protocol, Sel, BOOL};

use {ElementState, Event, KeyboardInput, VirtualKeyCode, WindowEvent, WindowId};
use platform::platform::events_loop::{DEVICE_ID, event_mods, Shared, to_virtual_key_code};
use platform::platform::input_client::*;
use platform::platform::util;
use platform::platform::window::{get_window_id, IdRef};

struct ViewState {
    window: id,
    shared: Weak<Shared>,
    queued_keycode: Option<VirtualKeyCode>,
}

pub fn new_view(window: id, shared: Weak<Shared>) -> IdRef {
    let state = ViewState { window, shared, queued_keycode: None };
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
        decl.add_method(sel!(keyUp:), key_up as extern fn(&Object, Sel, id));
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
    unsafe {
        let marked_text: id = *this.get_ivar("markedText");
        (marked_text.length() > 0) as i8
    }
}

extern fn marked_range(this: &Object, _sel: Sel) -> NSRange {
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
    EMPTY_RANGE
}

extern fn set_marked_text(
    this: &mut Object,
    _sel: Sel,
    string: id,
    _selected_range: NSRange,
    _replacement_range: NSRange,
) {
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
    unsafe {
        let marked_text: id = *this.get_ivar("markedText");
        let mutable_string = marked_text.mutableString();
        let _: () = msg_send![mutable_string, setString:""];
    }
}

extern fn valid_attributes_for_marked_text(_this: &Object, _sel: Sel) -> id {
    unsafe { msg_send![class("NSArray"), array] }
}

extern fn attributed_substring_for_proposed_range(
    _this: &Object,
    _sel: Sel,
    _range: NSRange,
    _actual_range: *mut c_void, // *mut NSRange
) -> id {
    nil
}

extern fn character_index_for_point(_this: &Object, _sel: Sel, _point: NSPoint) -> NSUInteger {
    0
}

extern fn first_rect_for_character_range(
    this: &Object,
    _sel: Sel,
    _range: NSRange,
    _actual_range: *mut c_void, // *mut NSRange
) -> NSRect {
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
        println!("insertText {:?}", slice);
        let string = str::from_utf8_unchecked(slice);

        // We don't need this now, but it's here if that changes.
        //let event: id = msg_send![class("NSApp"), currentEvent];

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
                .append(&mut events);
        }
    }
}

extern fn do_command_by_selector(this: &Object, _sel: Sel, command: Sel) {
    unsafe {
        let state_ptr: *mut c_void = *this.get_ivar("winitState");
        let state = &mut *(state_ptr as *mut ViewState);

        let shared = if let Some(shared) = state.shared.upgrade() {
            shared
        } else {
            return;
        };

        let event = if command == sel!(insertNewline:) {
            WindowEvent::ReceivedCharacter('\n')
        } else if command == sel!(noop:) {
            println!("noop");
            // O (insertNewlineIgnoringFieldEditor + moveBackward)
            // W (noop), E (moveToEndOfParagraph), R (noop), T (transpose), P (moveUp), U (noop), Y (yank)
            let character: u8 = match state.queued_keycode.take() {
                Some(VirtualKeyCode::C) => 0x03,
                Some(VirtualKeyCode::D) => 0x04,
                Some(VirtualKeyCode::V) => 0x16,
                Some(VirtualKeyCode::Z) => 0x1A,
                _ => return,
            };
            WindowEvent::ReceivedCharacter(character as char)
        } else {
            println!("doCommandBySelector {:?}", command);
            // Uncomment these lines if you love beeping sounds!
            //let next_responder: id = msg_send![this, nextResponder];
            //if next_responder != nil {
            //    let _: () = msg_send![next_responder, doCommandBySelector:command];
            //}
            return;
        };

        shared.pending_events
            .lock()
            .unwrap()
            .push_back(Event::WindowEvent {
                window_id: WindowId(get_window_id(state.window)),
                event,
            });
    }
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

        state.queued_keycode = virtual_keycode;

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

extern fn key_up(this: &Object, _sel: Sel, event: id) {
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
                    state: ElementState::Released,
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
