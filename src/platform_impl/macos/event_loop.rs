use std::{
    collections::VecDeque, marker::PhantomData, process,
};

use cocoa::{appkit::NSApp, base::{id, nil}, foundation::NSAutoreleasePool};

use {
    event::Event,
    event_loop::{ControlFlow, EventLoopClosed, EventLoopWindowTarget as RootWindowTarget},
};
use platform_impl::platform::{
    app::APP_CLASS, app_delegate::APP_DELEGATE_CLASS,
    app_state::AppState, monitor::{self, MonitorHandle},
    observer::setup_control_flow_observers, util::IdRef,
};

pub struct EventLoopWindowTarget<T: 'static> {
    _marker: PhantomData<T>,
}

impl<T> Default for EventLoopWindowTarget<T> {
    fn default() -> Self {
        EventLoopWindowTarget { _marker: PhantomData }
    }
}

pub struct EventLoop<T: 'static> {
    window_target: RootWindowTarget<T>,
    _delegate: IdRef,
}

impl<T> EventLoop<T> {
    pub fn new() -> Self {
        let delegate = unsafe {
            if !msg_send![class!(NSThread), isMainThread] {
                panic!("On macOS, `EventLoop` must be created on the main thread!");
            }

            // This must be done before `NSApp()` (equivalent to sending
            // `sharedApplication`) is called anywhere else, or we'll end up
            // with the wrong `NSApplication` class and the wrong thread could
            // be marked as main.
            let app: id = msg_send![APP_CLASS.0, sharedApplication];

            let delegate = IdRef::new(msg_send![APP_DELEGATE_CLASS.0, new]);
            let pool = NSAutoreleasePool::new(nil);
            let _: () = msg_send![app, setDelegate:*delegate];
            let _: () = msg_send![pool, drain];
            delegate
        };
        setup_control_flow_observers();
        EventLoop {
            window_target: RootWindowTarget::new(Default::default()),
            _delegate: delegate,
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

    pub fn window_target(&self) -> &RootWindowTarget<T> {
        &self.window_target
    }

    pub fn run<F>(self, callback: F) -> !
        where F: 'static + FnMut(Event<T>, &RootWindowTarget<T>, &mut ControlFlow),
    {
        unsafe {
            let _pool = NSAutoreleasePool::new(nil);
            let app = NSApp();
            assert_ne!(app, nil);
            AppState::set_callback(callback, self.window_target);
            let _: () = msg_send![app, run];
            AppState::exit();
            process::exit(0)
        }
    }

    pub fn run_return<F>(&mut self, _callback: F)
        where F: FnMut(Event<T>, &RootWindowTarget<T>, &mut ControlFlow),
    {
        unimplemented!();
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
    pub fn send_event(&self, _event: T) -> Result<(), EventLoopClosed> {
        unimplemented!();
    }
}
