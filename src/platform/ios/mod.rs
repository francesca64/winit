//! iOS support
//!
//! # Building app
//! To build ios app you will need rustc built for this targets:
//!
//!  - armv7-apple-ios
//!  - armv7s-apple-ios
//!  - i386-apple-ios
//!  - aarch64-apple-ios
//!  - x86_64-apple-ios
//!
//! Then
//!
//! ```
//! cargo build --target=...
//! ```
//! The simplest way to integrate your app into xcode environment is to build it
//! as a static library. Wrap your main function and export it.
//!
//! ```rust, ignore
//! #[no_mangle]
//! pub extern fn start_winit_app() {
//!     start_inner()
//! }
//!
//! fn start_inner() {
//!    ...
//! }
//!
//! ```
//!
//! Compile project and then drag resulting .a into Xcode project. Add winit.h to xcode.
//!
//! ```ignore
//! void start_winit_app();
//! ```
//!
//! Use start_winit_app inside your xcode's main function.
//!
//!
//! # App lifecycle and events
//!
//! iOS environment is very different from other platforms and you must be very
//! careful with it's events. Familiarize yourself with
//! [app lifecycle](https://developer.apple.com/library/ios/documentation/UIKit/Reference/UIApplicationDelegate_Protocol/).
//!
//!
//! This is how those event are represented in winit:
//!
//!  - applicationDidBecomeActive is Focused(true)
//!  - applicationWillResignActive is Focused(false)
//!  - applicationDidEnterBackground is Suspended(true)
//!  - applicationWillEnterForeground is Suspended(false)
//!  - applicationWillTerminate is Destroyed
//!
//! Keep in mind that after Destroyed event is received every attempt to draw with
//! opengl will result in segfault.
//!
//! Also note that app will not receive Destroyed event if suspended, it will be SIGKILL'ed

#![cfg(target_os = "ios")]

use std::{mem, ptr};
use std::collections::VecDeque;
use std::os::raw::*;

use objc::declare::ClassDecl;
use objc::runtime::{BOOL, Class, Object, Sel, YES};

use {
    CreationError,
    CursorState,
    Event,
    LogicalPosition,
    LogicalSize,
    MouseCursor,
    PhysicalPosition,
    PhysicalSize,
    WindowAttributes,
    WindowEvent,
    WindowId as RootEventId,
};
use events::{Touch, TouchPhase};
use window::MonitorId as RootMonitorId;

mod ffi;
use self::ffi::{
    CFTimeInterval,
    CFRunLoopRunInMode,
    CGFloat,
    CGPoint,
    CGRect,
    id,
    kCFRunLoopDefaultMode,
    kCFRunLoopRunHandledSource,
    longjmp,
    nil,
    NSString,
    setjmp,
    UIApplicationMain,
 };

static mut JMPBUF: [c_int; 27] = [0; 27];

#[derive(Debug, Clone)]
pub struct MonitorId;

pub struct Window {
    delegate_state: *mut DelegateState,
}

#[derive(Debug)]
struct DelegateState {
    events_queue: VecDeque<Event>,
    window: id,
    controller: id,
    size: LogicalSize,
    scale: f64,
}

impl DelegateState {
    #[inline]
    fn new(window: id, controller: id, size: LogicalSize, scale: f64) -> DelegateState {
        DelegateState {
            events_queue: VecDeque::new(),
            window,
            controller,
            size,
            scale,
        }
    }
}

impl MonitorId {
    fn get_uiscreen() -> id {
        unsafe { msg_send![Class::get("UIScreen").unwrap(), mainScreen] }
    }

    #[inline]
    pub fn get_name(&self) -> Option<String> {
        Some("Primary".to_string())
    }

    #[inline]
    pub fn get_dimensions(&self) -> PhysicalSize {
        let bounds: CGRect = unsafe { msg_send![MonitorId::get_uiscreen(), nativeBounds] };
        (bounds.size.width as f64, bounds.size.height as f64).into()
    }

    #[inline]
    pub fn get_position(&self) -> PhysicalPosition {
        // iOS assumes single screen
        (0, 0).into()
    }

    #[inline]
    pub fn get_hidpi_factor(&self) -> f64 {
        let scale: CGFloat = unsafe { msg_send![MonitorId::get_uiscreen(), nativeScale] };
        scale as f64
    }
}

pub struct EventsLoop {
    delegate_state: *mut DelegateState,
}

#[derive(Clone)]
pub struct EventsLoopProxy;

impl EventsLoop {
    pub fn new() -> EventsLoop {
        unsafe {
            if setjmp(mem::transmute(&mut JMPBUF)) != 0 {
                let app: id = msg_send![Class::get("UIApplication").unwrap(), sharedApplication];
                let delegate: id = msg_send![app, delegate];
                let state: *mut c_void = *(&*delegate).get_ivar("winitState");
                let delegate_state = state as *mut DelegateState;
                return EventsLoop { delegate_state };
            }
        }

        create_delegate_class();
        create_view_class();
        start_app();

        panic!("Couldn't create `UIApplication`!")
    }

    #[inline]
    pub fn get_available_monitors(&self) -> VecDeque<MonitorId> {
        let mut rb = VecDeque::with_capacity(1);
        rb.push_back(MonitorId);
        rb
    }

    #[inline]
    pub fn get_primary_monitor(&self) -> MonitorId {
        MonitorId
    }

    pub fn poll_events<F>(&mut self, mut callback: F)
        where F: FnMut(::Event)
    {
        unsafe {
            let state = &mut *self.delegate_state;

            if let Some(event) = state.events_queue.pop_front() {
                callback(event);
                return;
            }

            // jump hack, so we won't quit on willTerminate event before processing it
            if setjmp(mem::transmute(&mut JMPBUF)) != 0 {
                if let Some(event) = state.events_queue.pop_front() {
                    callback(event);
                    return;
                }
            }

            // run runloop
            let seconds: CFTimeInterval = 0.000002;
            while CFRunLoopRunInMode(kCFRunLoopDefaultMode, seconds, 1) == kCFRunLoopRunHandledSource {}

            if let Some(event) = state.events_queue.pop_front() {
                callback(event)
            }
        }
    }

    pub fn run_forever<F>(&mut self, mut callback: F)
        where F: FnMut(::Event) -> ::ControlFlow,
    {
        // Yeah that's a very bad implementation.
        loop {
            let mut control_flow = ::ControlFlow::Continue;
            self.poll_events(|e| {
                if let ::ControlFlow::Break = callback(e) {
                    control_flow = ::ControlFlow::Break;
                }
            });
            if let ::ControlFlow::Break = control_flow {
                break;
            }
            ::std::thread::sleep(::std::time::Duration::from_millis(5));
        }
    }

    pub fn create_proxy(&self) -> EventsLoopProxy {
        EventsLoopProxy
    }
}

impl EventsLoopProxy {
    pub fn wakeup(&self) -> Result<(), ::EventsLoopClosed> {
        unimplemented!()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId;

#[derive(Clone, Default)]
pub struct PlatformSpecificWindowBuilderAttributes;

impl Window {
    pub fn new(
        ev: &EventsLoop,
        _attributes: WindowAttributes,
        _pl_alltributes: PlatformSpecificWindowBuilderAttributes,
    ) -> Result<Window, CreationError> {
        Ok(Window { delegate_state: ev.delegate_state })
    }

    #[inline]
    pub fn set_title(&self, _title: &str) {
        // N/A
    }

    #[inline]
    pub fn show(&self) {
        // N/A
    }

    #[inline]
    pub fn hide(&self) {
        // N/A
    }

    #[inline]
    pub fn get_position(&self) -> Option<LogicalPosition> {
        // N/A
        None
    }

    #[inline]
    pub fn get_inner_position(&self) -> Option<LogicalPosition> {
        // N/A
        None
    }

    #[inline]
    pub fn set_position(&self, _position: LogicalPosition) {
        // N/A
    }

    #[inline]
    pub fn get_inner_size(&self) -> Option<LogicalSize> {
        unsafe { Some((&*self.delegate_state).size) }
    }

    #[inline]
    pub fn get_outer_size(&self) -> Option<LogicalSize> {
        self.get_inner_size()
    }

    #[inline]
    pub fn set_inner_size(&self, _size: LogicalSize) {
        // N/A
    }

    #[inline]
    pub fn set_min_dimensions(&self, _dimensions: Option<LogicalSize>) {
        // N/A
    }

    #[inline]
    pub fn set_max_dimensions(&self, _dimensions: Option<LogicalSize>) {
        // N/A
    }

    #[inline]
    pub fn set_cursor(&self, _cursor: MouseCursor) {
        // N/A
    }

    #[inline]
    pub fn set_cursor_state(&self, _cursor_state: CursorState) -> Result<(), String> {
        // N/A
        Ok(())
    }

    #[inline]
    pub fn get_hidpi_factor(&self) -> f64 {
        unsafe { (&*self.delegate_state) }.scale
    }

    #[inline]
    pub fn set_cursor_position(&self, _position: LogicalPosition) -> Result<(), ()> {
        // N/A
        Ok(())
    }

    #[inline]
    pub fn set_maximized(&self, _maximized: bool) {
        // N/A
        // iOS has single screen maximized apps so nothing to do
    }

    #[inline]
    pub fn set_fullscreen(&self, _monitor: Option<RootMonitorId>) {
        // N/A
        // iOS has single screen maximized apps so nothing to do
    }

    #[inline]
    pub fn set_decorations(&self, _decorations: bool) {
        // N/A
    }

    #[inline]
    pub fn set_always_on_top(&self, _always_on_top: bool) {
        // N/A
    }

    #[inline]
    pub fn set_window_icon(&self, _icon: Option<::Icon>) {
        // N/A
    }

    #[inline]
    pub fn set_ime_spot(&self, _logical_spot: LogicalPosition) {
        // N/A
    }

    #[inline]
    pub fn get_current_monitor(&self) -> RootMonitorId {
        RootMonitorId { inner: MonitorId }
    }

    #[inline]
    pub fn id(&self) -> WindowId {
        WindowId
    }
}

fn create_delegate_class() {
    extern fn did_finish_launching(this: &mut Object, _: Sel, _: id, _: id) -> BOOL {
        unsafe {
            let main_screen: id = msg_send![Class::get("UIScreen").unwrap(), mainScreen];
            let bounds: CGRect = msg_send![main_screen, bounds];
            let scale: CGFloat = msg_send![main_screen, nativeScale];

            let window: id = msg_send![Class::get("UIWindow").unwrap(), alloc];
            let window: id = msg_send![window, initWithFrame:bounds.clone()];

            let size = (bounds.size.width as f64, bounds.size.height as f64).into();

            let view_controller: id = msg_send![Class::get("MainViewController").unwrap(), alloc];
            let view_controller: id = msg_send![view_controller, init];

            let _: () = msg_send![window, setRootViewController:view_controller];
            let _: () = msg_send![window, makeKeyAndVisible];

            let state = Box::new(DelegateState::new(window, view_controller, size, scale as f64));
            let state_ptr: *mut DelegateState = mem::transmute(state);
            this.set_ivar("winitState", state_ptr as *mut c_void);

            let _: () = msg_send![this, performSelector:sel!(postLaunch:) withObject:nil afterDelay:0.0];
        }
        YES
    }

    extern fn post_launch(_: &Object, _: Sel, _: id) {
        unsafe { longjmp(mem::transmute(&mut JMPBUF),1); }
    }

    extern fn did_become_active(this: &Object, _: Sel, _: id) {
        unsafe {
            let state: *mut c_void = *this.get_ivar("winitState");
            let state = &mut *(state as *mut DelegateState);
            state.events_queue.push_back(Event::WindowEvent {
                window_id: RootEventId(WindowId),
                event: WindowEvent::Focused(true),
            });
        }
    }

    extern fn will_resign_active(this: &Object, _: Sel, _: id) {
        unsafe {
            let state: *mut c_void = *this.get_ivar("winitState");
            let state = &mut *(state as *mut DelegateState);
            state.events_queue.push_back(Event::WindowEvent {
                window_id: RootEventId(WindowId),
                event: WindowEvent::Focused(false),
            });
        }
    }

    extern fn will_enter_foreground(this: &Object, _: Sel, _: id) {
        unsafe {
            let state: *mut c_void = *this.get_ivar("winitState");
            let state = &mut *(state as *mut DelegateState);
            state.events_queue.push_back(Event::Suspended(false));
        }
    }

    extern fn did_enter_background(this: &Object, _: Sel, _: id) {
        unsafe {
            let state: *mut c_void = *this.get_ivar("winitState");
            let state = &mut *(state as *mut DelegateState);
            state.events_queue.push_back(Event::Suspended(true));
        }
    }

    extern fn will_terminate(this: &Object, _: Sel, _: id) {
        unsafe {
            let state: *mut c_void = *this.get_ivar("winitState");
            let state = &mut *(state as *mut DelegateState);
            // push event to the front to garantee that we'll process it
            // immidiatly after jump
            state.events_queue.push_front(Event::WindowEvent {
                window_id: RootEventId(WindowId),
                event: WindowEvent::Destroyed,
            });
            longjmp(mem::transmute(&mut JMPBUF),1);
        }
    }

    extern fn handle_touches(this: &Object, _: Sel, touches: id, _:id) {
        unsafe {
            let state: *mut c_void = *this.get_ivar("winitState");
            let state = &mut *(state as *mut DelegateState);

            let touches_enum: id = msg_send![touches, objectEnumerator];

            loop {
                let touch: id = msg_send![touches_enum, nextObject];
                if touch == nil {
                    break
                }
                let location: CGPoint = msg_send![touch, locationInView:nil];
                let touch_id = touch as u64;
                let phase: i32 = msg_send![touch, phase];

                state.events_queue.push_back(Event::WindowEvent {
                    window_id: RootEventId(WindowId),
                    event: WindowEvent::Touch(Touch {
                        device_id: DEVICE_ID,
                        id: touch_id,
                        location: (location.x as f64, location.y as f64).into(),
                        phase: match phase {
                            0 => TouchPhase::Started,
                            1 => TouchPhase::Moved,
                            // 2 is UITouchPhaseStationary and is not expected here
                            3 => TouchPhase::Ended,
                            4 => TouchPhase::Cancelled,
                            _ => panic!("unexpected touch phase: {:?}", phase)
                        }
                    }),
                });
            }
        }
    }

    let ui_responder = Class::get("UIResponder").unwrap();
    let mut decl = ClassDecl::new("AppDelegate", ui_responder).unwrap();

    unsafe {
        decl.add_method(sel!(application:didFinishLaunchingWithOptions:),
                        did_finish_launching as extern fn(&mut Object, Sel, id, id) -> BOOL);

        decl.add_method(sel!(applicationDidBecomeActive:),
                        did_become_active as extern fn(&Object, Sel, id));

        decl.add_method(sel!(applicationWillResignActive:),
                        will_resign_active as extern fn(&Object, Sel, id));

        decl.add_method(sel!(applicationWillEnterForeground:),
                        will_enter_foreground as extern fn(&Object, Sel, id));

        decl.add_method(sel!(applicationDidEnterBackground:),
                        did_enter_background as extern fn(&Object, Sel, id));

        decl.add_method(sel!(applicationWillTerminate:),
                        will_terminate as extern fn(&Object, Sel, id));


        decl.add_method(sel!(touchesBegan:withEvent:),
                        handle_touches as extern fn(this: &Object, _: Sel, _: id, _:id));

        decl.add_method(sel!(touchesMoved:withEvent:),
                        handle_touches as extern fn(this: &Object, _: Sel, _: id, _:id));

        decl.add_method(sel!(touchesEnded:withEvent:),
                        handle_touches as extern fn(this: &Object, _: Sel, _: id, _:id));

        decl.add_method(sel!(touchesCancelled:withEvent:),
                        handle_touches as extern fn(this: &Object, _: Sel, _: id, _:id));


        decl.add_method(sel!(postLaunch:),
                        post_launch as extern fn(&Object, Sel, id));

        decl.add_ivar::<*mut c_void>("winitState");

        decl.register();
    }
}

fn create_view_class() {
    let ui_view_controller = Class::get("UIViewController").unwrap();
    let decl = ClassDecl::new("MainViewController", ui_view_controller).unwrap();
    decl.register();
}

#[inline]
fn start_app() {
    unsafe {
        UIApplicationMain(0, ptr::null(), nil, NSString::alloc(nil).init_str("AppDelegate"));
    }
}

// Constant device ID, to be removed when this backend is updated to report real device IDs.
const DEVICE_ID: ::DeviceId = ::DeviceId(DeviceId);
