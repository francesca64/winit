use std::{f64, os::raw::c_void, sync::{Arc, Mutex, Weak}};

use cocoa::{
    appkit::{self, NSView, NSWindow},
    base::{id, nil},
    foundation::NSAutoreleasePool,
};
use objc::{runtime::{Class, Object, Sel, BOOL, YES, NO}, declare::ClassDecl};

use {
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    window::WindowId,
};
use platform_impl::platform::{
    event_loop::{EventLoopWindowTarget, PendingEvents, WindowList},
    util::{self, Access, IdRef},
    window::{get_window_id, UnownedWindow},
};

pub struct WindowDelegateState {
    nswindow: IdRef, // never changes
    nsview: IdRef, // never changes

    window: Weak<UnownedWindow>,
    pending_events: Weak<Mutex<PendingEvents>>,
    window_list: Weak<Mutex<WindowList>>,

    // TODO: It's possible for delegate methods to be called asynchronously,
    // causing data races / `RefCell` panics.

    // This is set when WindowBuilder::with_fullscreen was set,
    // see comments of `window_did_fail_to_enter_fullscreen`
    initial_fullscreen: bool,

    // During `windowDidResize`, we use this to only send Moved if the position changed.
    previous_position: Option<(f64, f64)>,

    // Used to prevent redundant events.
    previous_dpi_factor: f64,
}

impl WindowDelegateState {
    pub fn new<T: 'static>(
        window: &Arc<UnownedWindow>,
        elw_target: &EventLoopWindowTarget<T>,
        initial_fullscreen: bool,
    ) -> Self {
        let dpi_factor = window.get_hidpi_factor();

        let mut delegate_state = WindowDelegateState {
            nswindow: window.nswindow.clone(),
            nsview: window.nsview.clone(),
            window: Arc::downgrade(&window),
            pending_events: Arc::downgrade(&elw_target.pending_events),
            window_list: Arc::downgrade(&elw_target.window_list),
            initial_fullscreen,
            previous_position: None,
            previous_dpi_factor: dpi_factor,
        };

        if dpi_factor != 1.0 {
            delegate_state.emit_event(WindowEvent::HiDpiFactorChanged(dpi_factor));
            delegate_state.emit_resize_event();
        }

        delegate_state
    }

    fn with_window<F, T>(&mut self, callback: F) -> Option<T>
        where F: FnOnce(&UnownedWindow) -> T
    {
        self.window
            .upgrade()
            .map(|ref window| callback(window))
    }

    pub fn emit_event(&mut self, event: WindowEvent) {
        let event = Event::WindowEvent {
            window_id: WindowId(get_window_id(*self.nswindow)),
            event,
        };
        trace!("Locked pending events in `emit_event`");
        self.pending_events.access(|pending| pending.queue_event(event));
        trace!("Unlocked pending events in `emit_event`");
    }

    pub fn emit_resize_event(&mut self) {
        let rect = unsafe { NSView::frame(*self.nsview) };
        let size = LogicalSize::new(rect.size.width as f64, rect.size.height as f64);
        self.emit_event(WindowEvent::Resized(size));
    }

    fn emit_move_event(&mut self) {
        let rect = unsafe { NSWindow::frame(*self.nswindow) };
        let x = rect.origin.x as f64;
        let y = util::bottom_left_to_top_left(rect);
        let moved = self.previous_position != Some((x, y));
        if moved {
            self.previous_position = Some((x, y));
            self.emit_event(WindowEvent::Moved((x, y).into()));
        }
    }
}

pub struct WindowDelegate {
    state: Box<WindowDelegateState>,
    _this: IdRef,
}

impl WindowDelegate {
    pub fn new(state: WindowDelegateState) -> WindowDelegate {
        // Box the state so it will have a fixed address
        let mut state = Box::new(state);
        let state_ptr: *mut WindowDelegateState = &mut *state;
        unsafe {
            let delegate = IdRef::new(msg_send![WINDOW_DELEGATE_CLASS.0, new]);

            // setDelegate uses autorelease on objects,
            // so we need to use autorelease too.
            let autoreleasepool = NSAutoreleasePool::new(nil);

            (&mut **delegate).set_ivar("winitState", state_ptr as *mut c_void);
            let _: () = msg_send![*state.nswindow, setDelegate:*delegate];

            let _: () = msg_send![autoreleasepool, drain];

            WindowDelegate { state, _this: delegate }
        }
    }
}

impl Drop for WindowDelegate {
    fn drop(&mut self) {
        trace!("Dropping `WindowDelegate` ({:?})", self as *mut _);
        unsafe {
            // Nil the window's delegate so it doesn't still reference us
            // NOTE: setDelegate:nil at first retains the previous value,
            // and then autoreleases it, so autorelease pool is needed
            let autoreleasepool = NSAutoreleasePool::new(nil);
            let _: () = msg_send![*self.state.nswindow, setDelegate:nil];
            let _: () = msg_send![autoreleasepool, drain];
        }
    }
}

struct WindowDelegateClass(*const Class);
unsafe impl Send for WindowDelegateClass {}
unsafe impl Sync for WindowDelegateClass {}

lazy_static! {
    static ref WINDOW_DELEGATE_CLASS: WindowDelegateClass = unsafe {
        let superclass = class!(NSResponder);
        let mut decl = ClassDecl::new("WinitWindowDelegate", superclass).unwrap();

        decl.add_method(
            sel!(windowShouldClose:),
            window_should_close as extern fn(&Object, Sel, id) -> BOOL,
        );
        decl.add_method(
            sel!(windowWillClose:),
            window_will_close as extern fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(windowDidResize:),
            window_did_resize as extern fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(windowDidMove:),
            window_did_move as extern fn(&Object, Sel, id));
        decl.add_method(
            sel!(windowDidChangeScreen:),
            window_did_change_screen as extern fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(windowDidChangeBackingProperties:),
            window_did_change_backing_properties as extern fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(windowDidBecomeKey:),
            window_did_become_key as extern fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(windowDidResignKey:),
            window_did_resign_key as extern fn(&Object, Sel, id),
        );

        decl.add_method(
            sel!(draggingEntered:),
            dragging_entered as extern fn(&Object, Sel, id) -> BOOL,
        );
        decl.add_method(
            sel!(prepareForDragOperation:),
            prepare_for_drag_operation as extern fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(performDragOperation:),
            perform_drag_operation as extern fn(&Object, Sel, id) -> BOOL,
        );
        decl.add_method(
            sel!(concludeDragOperation:),
            conclude_drag_operation as extern fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(draggingExited:),
            dragging_exited as extern fn(&Object, Sel, id),
        );

        decl.add_method(
            sel!(windowDidEnterFullScreen:),
            window_did_enter_fullscreen as extern fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(windowWillEnterFullScreen:),
            window_will_enter_fullscreen as extern fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(windowDidExitFullScreen:),
            window_did_exit_fullscreen as extern fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(windowDidFailToEnterFullScreen:),
            window_did_fail_to_enter_fullscreen as extern fn(&Object, Sel, id),
        );

        decl.add_ivar::<*mut c_void>("winitState");

        WindowDelegateClass(decl.register())
    };
}

// This function is definitely unsafe, but labeling that would increase
// boilerplate and wouldn't really clarify anything...
fn with_state<F: FnOnce(&mut WindowDelegateState) -> T, T>(this: &Object, callback: F) {
    let state_ptr = unsafe {
        let state_ptr: *mut c_void = *this.get_ivar("winitState");
        &mut *(state_ptr as *mut WindowDelegateState)
    };
    callback(state_ptr);
}

// extern fn dealloc(this: &Object, _sel: Sel) {
//     with_state(this, |state| unsafe {
//         Box::from_raw(state as *mut WindowDelegateState);
//     });
// }

extern fn window_should_close(this: &Object, _: Sel, _: id) -> BOOL {
    trace!("Triggered `windowShouldClose`");
    with_state(this, |state| state.emit_event(WindowEvent::CloseRequested));
    trace!("Completed `windowShouldClose`");
    NO
}

extern fn window_will_close(this: &Object, _: Sel, _: id) {
    trace!("Triggered `windowWillClose`");
    with_state(this, |state| {
        state.emit_event(WindowEvent::Destroyed);
        state.window_list.access(|windows| {
            windows.remove_window(get_window_id(*state.nswindow));
        });
    });
    trace!("Completed `windowWillClose`");
}

extern fn window_did_resize(this: &Object, _: Sel, _: id) {
    trace!("Triggered `windowDidResize`");
    with_state(this, |state| {
        state.emit_resize_event();
        state.emit_move_event();
    });
    trace!("Completed `windowDidResize`");
}

// This won't be triggered if the move was part of a resize.
extern fn window_did_move(this: &Object, _: Sel, _: id) {
    trace!("Triggered `windowDidMove`");
    with_state(this, |state| {
        state.emit_move_event();
    });
    trace!("Completed `windowDidMove`");
}

extern fn window_did_change_screen(this: &Object, _: Sel, _: id) {
    trace!("Triggered `windowDidChangeScreen`");
    with_state(this, |state| {
        let dpi_factor = unsafe {
            NSWindow::backingScaleFactor(*state.nswindow)
         } as f64;
        if state.previous_dpi_factor != dpi_factor {
            state.previous_dpi_factor = dpi_factor;
            state.emit_event(WindowEvent::HiDpiFactorChanged(dpi_factor));
            state.emit_resize_event();
        }
    });
    trace!("Completed `windowDidChangeScreen`");
}

// This will always be called before `window_did_change_screen`.
extern fn window_did_change_backing_properties(this: &Object, _:Sel, _:id) {
    trace!("Triggered `windowDidChangeBackingProperties`");
    with_state(this, |state| {
        let dpi_factor = unsafe {
            NSWindow::backingScaleFactor(*state.nswindow)
        } as f64;
        if state.previous_dpi_factor != dpi_factor {
            state.previous_dpi_factor = dpi_factor;
            state.emit_event(WindowEvent::HiDpiFactorChanged(dpi_factor));
            state.emit_resize_event();
        }
    });
    trace!("Completed `windowDidChangeBackingProperties`");
}

extern fn window_did_become_key(this: &Object, _: Sel, _: id) {
    trace!("Triggered `windowDidBecomeKey`");
    with_state(this, |state| {
        // TODO: center the cursor if the window had mouse grab when it
        // lost focus
        state.emit_event(WindowEvent::Focused(true));
    });
    trace!("Completed `windowDidBecomeKey`");
}

extern fn window_did_resign_key(this: &Object, _: Sel, _: id) {
    trace!("Triggered `windowDidResignKey`");
    with_state(this, |state| {
        state.emit_event(WindowEvent::Focused(false));
    });
    trace!("Completed `windowDidResignKey`");
}

/// Invoked when the dragged image enters destination bounds or frame
extern fn dragging_entered(this: &Object, _: Sel, sender: id) -> BOOL {
    trace!("Triggered `draggingEntered`");

    use cocoa::appkit::NSPasteboard;
    use cocoa::foundation::NSFastEnumeration;
    use std::path::PathBuf;

    let pb: id = unsafe { msg_send![sender, draggingPasteboard] };
    let filenames = unsafe { NSPasteboard::propertyListForType(pb, appkit::NSFilenamesPboardType) };

    for file in unsafe { filenames.iter() } {
        use cocoa::foundation::NSString;
        use std::ffi::CStr;

        unsafe {
            let f = NSString::UTF8String(file);
            let path = CStr::from_ptr(f).to_string_lossy().into_owned();

            with_state(this, |state| {
                state.emit_event(WindowEvent::HoveredFile(PathBuf::from(path)));
            });
        }
    };

    trace!("Completed `draggingEntered`");
    YES
}

/// Invoked when the image is released
extern fn prepare_for_drag_operation(_: &Object, _: Sel, _: id) {
    trace!("Triggered `prepareForDragOperation`");
    trace!("Completed `prepareForDragOperation`");
}

/// Invoked after the released image has been removed from the screen
extern fn perform_drag_operation(this: &Object, _: Sel, sender: id) -> BOOL {
    trace!("Triggered `performDragOperation`");

    use cocoa::appkit::NSPasteboard;
    use cocoa::foundation::NSFastEnumeration;
    use std::path::PathBuf;

    let pb: id = unsafe { msg_send![sender, draggingPasteboard] };
    let filenames = unsafe { NSPasteboard::propertyListForType(pb, appkit::NSFilenamesPboardType) };

    for file in unsafe { filenames.iter() } {
        use cocoa::foundation::NSString;
        use std::ffi::CStr;

        unsafe {
            let f = NSString::UTF8String(file);
            let path = CStr::from_ptr(f).to_string_lossy().into_owned();

            with_state(this, |state| {
                state.emit_event(WindowEvent::DroppedFile(PathBuf::from(path)));
            });
        }
    };

    trace!("Completed `performDragOperation`");
    YES
}

/// Invoked when the dragging operation is complete
extern fn conclude_drag_operation(_: &Object, _: Sel, _: id) {
    trace!("Triggered `concludeDragOperation`");
    trace!("Completed `concludeDragOperation`");
}

/// Invoked when the dragging operation is cancelled
extern fn dragging_exited(this: &Object, _: Sel, _: id) {
    trace!("Triggered `draggingExited`");
    with_state(this, |state| state.emit_event(WindowEvent::HoveredFileCancelled));
    trace!("Completed `draggingExited`");
}

/// Invoked when before enter fullscreen
extern fn window_will_enter_fullscreen(this: &Object, _: Sel, _: id) {
    trace!("Triggered `windowWillEnterFullscreen`");
    with_state(this, |state| state.with_window(|window| {
        trace!("Locked shared state in `window_will_enter_fullscreen`");
        window.shared_state.lock().unwrap().maximized = window.is_zoomed();
        trace!("Unlocked shared state in `window_will_enter_fullscreen`");
    }));
    trace!("Completed `windowWillEnterFullscreen`");
}

/// Invoked when entered fullscreen
extern fn window_did_enter_fullscreen(this: &Object, _: Sel, _: id) {
    trace!("Triggered `windowDidEnterFullscreen`");
    with_state(this, |state| {
        state.with_window(|window| {
            let monitor = window.get_current_monitor();
            trace!("Locked shared state in `window_did_enter_fullscreen`");
            window.shared_state.lock().unwrap().fullscreen = Some(monitor);
            trace!("Unlocked shared state in `window_will_enter_fullscreen`");
        });
        state.initial_fullscreen = false;
    });
    trace!("Completed `windowDidEnterFullscreen`");
}

/// Invoked when exited fullscreen
extern fn window_did_exit_fullscreen(this: &Object, _: Sel, _: id) {
    trace!("Triggered `windowDidExitFullscreen`");
    with_state(this, |state| state.with_window(|window| {
        window.restore_state_from_fullscreen();
    }));
    trace!("Completed `windowDidExitFullscreen`");
}

/// Invoked when fail to enter fullscreen
///
/// When this window launch from a fullscreen app (e.g. launch from VS Code
/// terminal), it creates a new virtual destkop and a transition animation.
/// This animation takes one second and cannot be disable without
/// elevated privileges. In this animation time, all toggleFullscreen events
/// will be failed. In this implementation, we will try again by using
/// performSelector:withObject:afterDelay: until window_did_enter_fullscreen.
/// It should be fine as we only do this at initialzation (i.e with_fullscreen
/// was set).
///
/// From Apple doc:
/// In some cases, the transition to enter full-screen mode can fail,
/// due to being in the midst of handling some other animation or user gesture.
/// This method indicates that there was an error, and you should clean up any
/// work you may have done to prepare to enter full-screen mode.
extern fn window_did_fail_to_enter_fullscreen(this: &Object, _: Sel, _: id) {
    trace!("Triggered `windowDidFailToEnterFullscreen`");
    with_state(this, |state| {
        if state.initial_fullscreen {
            let _: () = unsafe { msg_send![*state.nswindow,
                performSelector:sel!(toggleFullScreen:)
                withObject:nil
                afterDelay: 0.5
            ] };
        } else {
            state.with_window(|window| window.restore_state_from_fullscreen());
        }
    });
    trace!("Completed `windowDidFailToEnterFullscreen`");
}
