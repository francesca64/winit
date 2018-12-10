use std::{
    collections::VecDeque, hint::unreachable_unchecked,
    marker::PhantomData, os::raw::*, process::exit, sync::{Arc, Mutex, Weak},
};

use cocoa::{
    appkit::{
        self, NSApp, NSApplication, NSApplicationDefined, NSEvent, NSEventMask,
        NSEventModifierFlags, NSEventPhase, NSEventSubtype, NSView, NSWindow,
    },
    base::{BOOL, id, nil, NO, YES},
    foundation::{NSAutoreleasePool, NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize},
};

use {
    event::{
        self, DeviceEvent, ElementState, Event, KeyboardInput,
        ModifiersState, TouchPhase, WindowEvent,
    },
    event_loop::{ControlFlow, EventLoopClosed, EventLoopWindowTarget as RootELW},
    window,
};
use platform_impl::platform::{
    DEVICE_ID, monitor::{self, MonitorHandle}, window::UnownedWindow,
};

#[derive(Default)]
struct Modifiers {
    shift_pressed: bool,
    ctrl_pressed: bool,
    win_pressed: bool,
    alt_pressed: bool,
}

// Change to `!` once stable
pub enum Never {}

impl Event<Never> {
    fn userify<T: 'static>(self) -> Event<T> {
        self.map_nonuser_event()
            // `Never` can't be constructed, so the `UserEvent` variant can't
            // be present here.
            .unwrap_or_else(|_| unsafe { unreachable_unchecked() })
    }
}

// State shared between the `EventLoop` and its registered windows and delegates.
#[derive(Default)]
pub struct PendingEvents {
    pending: VecDeque<Event<Never>>,
}

impl PendingEvents {
    pub fn queue_event(&mut self, event: Event<Never>) {
        self.pending.push_back(event);
    }

    pub fn queue_events(&mut self, mut events: VecDeque<Event<Never>>) {
        self.pending.append(&mut events);
    }

    fn process_events<F: FnMut(Event<T>), T: 'static>(&mut self, mut callback: F) {
        for event in self.pending.drain(0..) {
            callback(event.userify());
        }
    }
}

#[derive(Default)]
pub struct WindowList {
    windows: Vec<Weak<UnownedWindow>>,
}

impl WindowList {
    pub fn insert_window(&mut self, window: Weak<UnownedWindow>) {
        self.windows.push(window);
    }

    // Removes the window with the given `Id` from the `windows` list.
    //
    // This is called in response to `windowWillClose`.
    pub fn remove_window(&mut self, id: super::window::Id) {
        self.windows
            .retain(|window| window
                .upgrade()
                .map(|window| window.id() != id)
                .unwrap_or(false)
            );
    }
}

pub struct EventLoopWindowTarget<T: 'static> {
    pub pending_events: Arc<Mutex<PendingEvents>>,
    pub window_list: Arc<Mutex<WindowList>>,
    _marker: PhantomData<T>,
}

impl<T> Default for EventLoopWindowTarget<T> {
    fn default() -> Self {
        EventLoopWindowTarget {
            pending_events: Default::default(),
            window_list: Default::default(),
            _marker: PhantomData,
        }
    }
}

pub struct EventLoop<T: 'static> {
    elw_target: RootELW<T>,
    modifiers: Modifiers,
}

impl<T> EventLoop<T> {
    pub fn new() -> Self {
        // Mark this thread as the main thread of the Cocoa event system.
        //
        // This must be done before any worker threads get a chance to call it
        // (e.g., via `EventLoopProxy::wakeup()`), causing a wrong thread to be
        // marked as the main thread.
        unsafe { NSApp() };
        EventLoop {
            elw_target: RootELW::new(Default::default()),
            modifiers: Default::default(),
        }
    }

    #[inline]
    pub fn get_available_monitors(&self) -> VecDeque<MonitorHandle> {
        monitor::get_available_monitors()
    }

    #[inline]
    pub fn get_primary_monitor(&self) -> MonitorHandle {
        monitor::get_primary_monitor()
    }

    pub fn window_target(&self) -> &RootELW<T> {
        &self.elw_target
    }

    pub fn run<F>(mut self, mut callback: F) -> !
        where F: 'static + FnMut(Event<T>, &RootELW<T>, &mut ControlFlow),
    {
        unsafe {
            if !msg_send![class!(NSThread), isMainThread] {
                panic!("Events can only be polled from the main thread on macOS");
            }
        }

        let mut control_flow = Default::default();

        loop {
            self.elw_target.inner.pending_events
                .lock()
                .unwrap()
                .process_events(|event| {
                    callback(event, self.window_target(), &mut control_flow);
                });

            if let ControlFlow::Exit = control_flow {
                exit(0);
            }

            let maybe_event = unsafe {
                let pool = NSAutoreleasePool::new(nil);

                // Wait for the next event. Note that this function blocks during resize.
                let ns_event = NSApp().nextEventMatchingMask_untilDate_inMode_dequeue_(
                    NSEventMask::NSAnyEventMask.bits() | NSEventMask::NSEventMaskPressure.bits(),
                    NSDate::distantFuture(nil),
                    NSDefaultRunLoopMode,
                    YES,
                );

                let maybe_event = self.translate_event(ns_event);

                // Release the pool before calling the top callback in case the user calls either
                // `run_forever` or `poll_events` within the callback.
                let _: () = msg_send![pool, release];

                maybe_event
            };

            if let Some(event) = maybe_event {
                callback(event.userify(), self.window_target(), &mut control_flow);
                if let ControlFlow::Exit = control_flow {
                    exit(0);
                }
            }
        }
    }

    pub fn run_return<F>(&mut self, _callback: F)
        where F: FnMut(Event<T>, &RootELW<T>, &mut ControlFlow),
    {
        unimplemented!();
    }

    // Converts an `NSEvent` to a winit `Event`.
    unsafe fn translate_event(&mut self, ns_event: id) -> Option<Event<Never>> {
        if ns_event == nil {
            return None;
        }

        // FIXME: Despite not being documented anywhere, an `NSEvent` is produced when a user opens
        // Spotlight while the NSApplication is in focus. This `NSEvent` produces a `NSEventType`
        // with value `21`. This causes a SEGFAULT as soon as we try to match on the `NSEventType`
        // enum as there is no variant associated with the value. Thus, we return early if this
        // sneaky event occurs. If someone does find some documentation on this, please fix this by
        // adding an appropriate variant to the `NSEventType` enum in the cocoa-rs crate.
        if ns_event.eventType() as u64 == 21 {
            return None;
        }

        let event_type = ns_event.eventType();
        let ns_window = ns_event.window();
        let window_id = super::window::get_window_id(ns_window);

        // FIXME: Document this. Why do we do this? Seems like it passes on events to window/app.
        // If we don't do this, window does not become main for some reason.
        NSApp().sendEvent_(ns_event);

        let windows = self.elw_target.inner.window_list.lock().unwrap();
        let maybe_window = (*windows)
            .windows
            .iter()
            .filter_map(Weak::upgrade)
            .find(|window| window_id == window.id());

        let into_event = |window_event| Event::WindowEvent {
            window_id: window::WindowId(window_id),
            event: window_event,
        };

        // Returns `Some` window if one of our windows is the key window.
        let maybe_key_window = || (*windows)
            .windows
            .iter()
            .filter_map(Weak::upgrade)
            .find(|window| {
                let is_key_window: BOOL = msg_send![*window.nswindow, isKeyWindow];
                is_key_window == YES
            });

        match event_type {
            // https://github.com/glfw/glfw/blob/50eccd298a2bbc272b4977bd162d3e4b55f15394/src/cocoa_window.m#L881
            appkit::NSKeyUp  => {
                if let Some(key_window) = maybe_key_window() {
                    if event_mods(ns_event).logo {
                        let _: () = msg_send![*key_window.nswindow, sendEvent:ns_event];
                    }
                }
                None
            },
            // similar to above, but for `<Cmd-.>`, the keyDown is suppressed instead of the
            // KeyUp, and the above trick does not appear to work.
            appkit::NSKeyDown => {
                let modifiers = event_mods(ns_event);
                let keycode = NSEvent::keyCode(ns_event);
                if modifiers.logo && keycode == 47 {
                    modifier_event(ns_event, NSEventModifierFlags::NSCommandKeyMask, false)
                        .map(into_event)
                } else {
                    None
                }
            },
            appkit::NSFlagsChanged => {
                let mut events = VecDeque::new();

                if let Some(window_event) = modifier_event(
                    ns_event,
                    NSEventModifierFlags::NSShiftKeyMask,
                    self.modifiers.shift_pressed,
                ) {
                    self.modifiers.shift_pressed = !self.modifiers.shift_pressed;
                    events.push_back(into_event(window_event));
                }

                if let Some(window_event) = modifier_event(
                    ns_event,
                    NSEventModifierFlags::NSControlKeyMask,
                    self.modifiers.ctrl_pressed,
                ) {
                    self.modifiers.ctrl_pressed = !self.modifiers.ctrl_pressed;
                    events.push_back(into_event(window_event));
                }

                if let Some(window_event) = modifier_event(
                    ns_event,
                    NSEventModifierFlags::NSCommandKeyMask,
                    self.modifiers.win_pressed,
                ) {
                    self.modifiers.win_pressed = !self.modifiers.win_pressed;
                    events.push_back(into_event(window_event));
                }

                if let Some(window_event) = modifier_event(
                    ns_event,
                    NSEventModifierFlags::NSAlternateKeyMask,
                    self.modifiers.alt_pressed,
                ) {
                    self.modifiers.alt_pressed = !self.modifiers.alt_pressed;
                    events.push_back(into_event(window_event));
                }

                let event = events.pop_front();
                self.elw_target.inner.pending_events
                    .lock()
                    .unwrap()
                    .queue_events(events);
                event
            },

            appkit::NSMouseEntered => {
                let window = match maybe_window.or_else(maybe_key_window) {
                    Some(window) => window,
                    None => return None,
                };

                let window_point = ns_event.locationInWindow();
                let view_point = if ns_window == nil {
                    let ns_size = NSSize::new(0.0, 0.0);
                    let ns_rect = NSRect::new(window_point, ns_size);
                    let window_rect = window.nswindow.convertRectFromScreen_(ns_rect);
                    window.nsview.convertPoint_fromView_(window_rect.origin, nil)
                } else {
                    window.nsview.convertPoint_fromView_(window_point, nil)
                };

                let view_rect = NSView::frame(*window.nsview);
                let x = view_point.x as f64;
                let y = (view_rect.size.height - view_point.y) as f64;
                let window_event = WindowEvent::CursorMoved {
                    device_id: DEVICE_ID,
                    position: (x, y).into(),
                    modifiers: event_mods(ns_event),
                };
                let event = Event::WindowEvent { window_id: window::WindowId(window.id()), event: window_event };
                self.elw_target.inner.pending_events
                    .lock()
                    .unwrap()
                    .queue_event(event);
                Some(into_event(WindowEvent::CursorEntered { device_id: DEVICE_ID }))
            },
            appkit::NSMouseExited => { Some(into_event(WindowEvent::CursorLeft { device_id: DEVICE_ID })) },

            appkit::NSMouseMoved |
            appkit::NSLeftMouseDragged |
            appkit::NSOtherMouseDragged |
            appkit::NSRightMouseDragged => {
                // If the mouse movement was on one of our windows, use it.
                // Otherwise, if one of our windows is the key window (receiving input), use it.
                // Otherwise, return `None`.
                match maybe_window.or_else(maybe_key_window) {
                    Some(_window) => (),
                    None => return None,
                }

                let mut events = VecDeque::with_capacity(3);

                let delta_x = ns_event.deltaX() as f64;
                if delta_x != 0.0 {
                    let motion_event = DeviceEvent::Motion { axis: 0, value: delta_x };
                    let event = Event::DeviceEvent { device_id: DEVICE_ID, event: motion_event };
                    events.push_back(event);
                }

                let delta_y = ns_event.deltaY() as f64;
                if delta_y != 0.0 {
                    let motion_event = DeviceEvent::Motion { axis: 1, value: delta_y };
                    let event = Event::DeviceEvent { device_id: DEVICE_ID, event: motion_event };
                    events.push_back(event);
                }

                if delta_x != 0.0 || delta_y != 0.0 {
                    let motion_event = DeviceEvent::MouseMotion { delta: (delta_x, delta_y) };
                    let event = Event::DeviceEvent { device_id: DEVICE_ID, event: motion_event };
                    events.push_back(event);
                }

                let event = events.pop_front();
                self.elw_target.inner.pending_events
                    .lock()
                    .unwrap()
                    .queue_events(events);
                event
            },

            appkit::NSScrollWheel => {
                // If none of the windows received the scroll, return `None`.
                if maybe_window.is_none() {
                    return None;
                }

                use event::MouseScrollDelta::{LineDelta, PixelDelta};
                let delta = if ns_event.hasPreciseScrollingDeltas() == YES {
                    PixelDelta((
                        ns_event.scrollingDeltaX() as f64,
                        ns_event.scrollingDeltaY() as f64,
                    ).into())
                } else {
                    // TODO: This is probably wrong
                    LineDelta(
                        ns_event.scrollingDeltaX() as f32,
                        ns_event.scrollingDeltaY() as f32,
                    )
                };
                let phase = match ns_event.phase() {
                    NSEventPhase::NSEventPhaseMayBegin | NSEventPhase::NSEventPhaseBegan => TouchPhase::Started,
                    NSEventPhase::NSEventPhaseEnded => TouchPhase::Ended,
                    _ => TouchPhase::Moved,
                };
                self.elw_target.inner.pending_events.lock().unwrap().queue_event(Event::DeviceEvent {
                    device_id: DEVICE_ID,
                    event: DeviceEvent::MouseWheel {
                        delta: if ns_event.hasPreciseScrollingDeltas() == YES {
                            PixelDelta((
                                ns_event.scrollingDeltaX() as f64,
                                ns_event.scrollingDeltaY() as f64,
                            ).into())
                        } else {
                            LineDelta(
                                ns_event.scrollingDeltaX() as f32,
                                ns_event.scrollingDeltaY() as f32,
                            )
                        },
                    }
                });
                let window_event = WindowEvent::MouseWheel {
                    device_id: DEVICE_ID,
                    delta,
                    phase,
                    modifiers: event_mods(ns_event),
                };
                Some(into_event(window_event))
            },

            appkit::NSEventTypePressure => {
                let pressure = ns_event.pressure();
                let stage = ns_event.stage();
                let window_event = WindowEvent::TouchpadPressure {
                    device_id: DEVICE_ID,
                    pressure,
                    stage,
                };
                Some(into_event(window_event))
            },

            NSApplicationDefined => match ns_event.subtype() {
                NSEventSubtype::NSApplicationActivatedEventType => {
                    unimplemented!();
                },
                _ => None,
            },

            _  => None,
        }
    }

    pub fn create_proxy(&self) -> Proxy<T> {
        Proxy::default()
    }
}

#[derive(Clone)]
pub struct Proxy<T> {
    _marker: PhantomData<T>,
}

impl<T> Default for Proxy<T> {
    fn default() -> Self {
        Proxy { _marker: PhantomData }
    }
}

impl<T> Proxy<T> {
    #[allow(unreachable_code)]
    pub fn send_event(&self, event: T) -> Result<(), EventLoopClosed> {
        unimplemented!();
        // Awaken the event loop by triggering `NSApplicationActivatedEventType`.
        unsafe {
            let pool = NSAutoreleasePool::new(nil);
            let event =
                NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2_(
                    nil,
                    NSApplicationDefined,
                    NSPoint::new(0.0, 0.0),
                    NSEventModifierFlags::empty(),
                    0.0,
                    0,
                    nil,
                    NSEventSubtype::NSApplicationActivatedEventType,
                    0,
                    0,
                );
            NSApp().postEvent_atStart_(event, NO);
            NSAutoreleasePool::drain(pool);
        }
        Ok(())
    }
}

pub fn to_virtual_key_code(code: c_ushort) -> Option<event::VirtualKeyCode> {
    Some(match code {
        0x00 => event::VirtualKeyCode::A,
        0x01 => event::VirtualKeyCode::S,
        0x02 => event::VirtualKeyCode::D,
        0x03 => event::VirtualKeyCode::F,
        0x04 => event::VirtualKeyCode::H,
        0x05 => event::VirtualKeyCode::G,
        0x06 => event::VirtualKeyCode::Z,
        0x07 => event::VirtualKeyCode::X,
        0x08 => event::VirtualKeyCode::C,
        0x09 => event::VirtualKeyCode::V,
        //0x0a => World 1,
        0x0b => event::VirtualKeyCode::B,
        0x0c => event::VirtualKeyCode::Q,
        0x0d => event::VirtualKeyCode::W,
        0x0e => event::VirtualKeyCode::E,
        0x0f => event::VirtualKeyCode::R,
        0x10 => event::VirtualKeyCode::Y,
        0x11 => event::VirtualKeyCode::T,
        0x12 => event::VirtualKeyCode::Key1,
        0x13 => event::VirtualKeyCode::Key2,
        0x14 => event::VirtualKeyCode::Key3,
        0x15 => event::VirtualKeyCode::Key4,
        0x16 => event::VirtualKeyCode::Key6,
        0x17 => event::VirtualKeyCode::Key5,
        0x18 => event::VirtualKeyCode::Equals,
        0x19 => event::VirtualKeyCode::Key9,
        0x1a => event::VirtualKeyCode::Key7,
        0x1b => event::VirtualKeyCode::Minus,
        0x1c => event::VirtualKeyCode::Key8,
        0x1d => event::VirtualKeyCode::Key0,
        0x1e => event::VirtualKeyCode::RBracket,
        0x1f => event::VirtualKeyCode::O,
        0x20 => event::VirtualKeyCode::U,
        0x21 => event::VirtualKeyCode::LBracket,
        0x22 => event::VirtualKeyCode::I,
        0x23 => event::VirtualKeyCode::P,
        0x24 => event::VirtualKeyCode::Return,
        0x25 => event::VirtualKeyCode::L,
        0x26 => event::VirtualKeyCode::J,
        0x27 => event::VirtualKeyCode::Apostrophe,
        0x28 => event::VirtualKeyCode::K,
        0x29 => event::VirtualKeyCode::Semicolon,
        0x2a => event::VirtualKeyCode::Backslash,
        0x2b => event::VirtualKeyCode::Comma,
        0x2c => event::VirtualKeyCode::Slash,
        0x2d => event::VirtualKeyCode::N,
        0x2e => event::VirtualKeyCode::M,
        0x2f => event::VirtualKeyCode::Period,
        0x30 => event::VirtualKeyCode::Tab,
        0x31 => event::VirtualKeyCode::Space,
        0x32 => event::VirtualKeyCode::Grave,
        0x33 => event::VirtualKeyCode::Back,
        //0x34 => unkown,
        0x35 => event::VirtualKeyCode::Escape,
        0x36 => event::VirtualKeyCode::LWin,
        0x37 => event::VirtualKeyCode::RWin,
        0x38 => event::VirtualKeyCode::LShift,
        //0x39 => Caps lock,
        0x3a => event::VirtualKeyCode::LAlt,
        0x3b => event::VirtualKeyCode::LControl,
        0x3c => event::VirtualKeyCode::RShift,
        0x3d => event::VirtualKeyCode::RAlt,
        0x3e => event::VirtualKeyCode::RControl,
        //0x3f => Fn key,
        0x40 => event::VirtualKeyCode::F17,
        0x41 => event::VirtualKeyCode::Decimal,
        //0x42 -> unkown,
        0x43 => event::VirtualKeyCode::Multiply,
        //0x44 => unkown,
        0x45 => event::VirtualKeyCode::Add,
        //0x46 => unkown,
        0x47 => event::VirtualKeyCode::Numlock,
        //0x48 => KeypadClear,
        0x49 => event::VirtualKeyCode::VolumeUp,
        0x4a => event::VirtualKeyCode::VolumeDown,
        0x4b => event::VirtualKeyCode::Divide,
        0x4c => event::VirtualKeyCode::NumpadEnter,
        //0x4d => unkown,
        0x4e => event::VirtualKeyCode::Subtract,
        0x4f => event::VirtualKeyCode::F18,
        0x50 => event::VirtualKeyCode::F19,
        0x51 => event::VirtualKeyCode::NumpadEquals,
        0x52 => event::VirtualKeyCode::Numpad0,
        0x53 => event::VirtualKeyCode::Numpad1,
        0x54 => event::VirtualKeyCode::Numpad2,
        0x55 => event::VirtualKeyCode::Numpad3,
        0x56 => event::VirtualKeyCode::Numpad4,
        0x57 => event::VirtualKeyCode::Numpad5,
        0x58 => event::VirtualKeyCode::Numpad6,
        0x59 => event::VirtualKeyCode::Numpad7,
        0x5a => event::VirtualKeyCode::F20,
        0x5b => event::VirtualKeyCode::Numpad8,
        0x5c => event::VirtualKeyCode::Numpad9,
        //0x5d => unkown,
        //0x5e => unkown,
        //0x5f => unkown,
        0x60 => event::VirtualKeyCode::F5,
        0x61 => event::VirtualKeyCode::F6,
        0x62 => event::VirtualKeyCode::F7,
        0x63 => event::VirtualKeyCode::F3,
        0x64 => event::VirtualKeyCode::F8,
        0x65 => event::VirtualKeyCode::F9,
        //0x66 => unkown,
        0x67 => event::VirtualKeyCode::F11,
        //0x68 => unkown,
        0x69 => event::VirtualKeyCode::F13,
        0x6a => event::VirtualKeyCode::F16,
        0x6b => event::VirtualKeyCode::F14,
        //0x6c => unkown,
        0x6d => event::VirtualKeyCode::F10,
        //0x6e => unkown,
        0x6f => event::VirtualKeyCode::F12,
        //0x70 => unkown,
        0x71 => event::VirtualKeyCode::F15,
        0x72 => event::VirtualKeyCode::Insert,
        0x73 => event::VirtualKeyCode::Home,
        0x74 => event::VirtualKeyCode::PageUp,
        0x75 => event::VirtualKeyCode::Delete,
        0x76 => event::VirtualKeyCode::F4,
        0x77 => event::VirtualKeyCode::End,
        0x78 => event::VirtualKeyCode::F2,
        0x79 => event::VirtualKeyCode::PageDown,
        0x7a => event::VirtualKeyCode::F1,
        0x7b => event::VirtualKeyCode::Left,
        0x7c => event::VirtualKeyCode::Right,
        0x7d => event::VirtualKeyCode::Down,
        0x7e => event::VirtualKeyCode::Up,
        //0x7f =>  unkown,

        0xa => event::VirtualKeyCode::Caret,
        _ => return None,
    })
}

pub fn check_additional_virtual_key_codes(
    s: &Option<String>
) -> Option<event::VirtualKeyCode> {
    if let &Some(ref s) = s {
        if let Some(ch) = s.encode_utf16().next() {
            return Some(match ch {
                0xf718 => event::VirtualKeyCode::F21,
                0xf719 => event::VirtualKeyCode::F22,
                0xf71a => event::VirtualKeyCode::F23,
                0xf71b => event::VirtualKeyCode::F24,
                _ => return None,
            })
        }
    }
    None
}

pub fn event_mods(event: id) -> ModifiersState {
    let flags = unsafe {
        NSEvent::modifierFlags(event)
    };
    ModifiersState {
        shift: flags.contains(NSEventModifierFlags::NSShiftKeyMask),
        ctrl: flags.contains(NSEventModifierFlags::NSControlKeyMask),
        alt: flags.contains(NSEventModifierFlags::NSAlternateKeyMask),
        logo: flags.contains(NSEventModifierFlags::NSCommandKeyMask),
    }
}

unsafe fn modifier_event(
    ns_event: id,
    keymask: NSEventModifierFlags,
    was_key_pressed: bool,
) -> Option<WindowEvent> {
    if !was_key_pressed && NSEvent::modifierFlags(ns_event).contains(keymask)
    || was_key_pressed && !NSEvent::modifierFlags(ns_event).contains(keymask) {
        let state = if was_key_pressed {
            ElementState::Released
        } else {
            ElementState::Pressed
        };
        let keycode = NSEvent::keyCode(ns_event);
        let scancode = keycode as u32;
        let virtual_keycode = to_virtual_key_code(keycode);
        Some(WindowEvent::KeyboardInput {
            device_id: DEVICE_ID,
            input: KeyboardInput {
                state,
                scancode,
                virtual_keycode,
                modifiers: event_mods(ns_event),
            },
        })
    } else {
        None
    }
}
