use std::{
    self, collections::VecDeque, fmt::{self, Debug, Formatter},
    hint::unreachable_unchecked, mem, sync::{Mutex, MutexGuard},
};

use cocoa::{appkit::NSApp, base::nil};

use {
    event::{Event, StartCause},
    event_loop::{ControlFlow, EventLoopWindowTarget as RootWindowTarget},
};
use platform_impl::platform::{observer::EventLoopWaker, util::Never};

lazy_static! {
    static ref HANDLER: Mutex<Handler> = Default::default();
    static ref EVENTS: Mutex<VecDeque<Event<Never>>> = Default::default();
}

impl Event<Never> {
    fn userify<T: 'static>(self) -> Event<T> {
        self.map_nonuser_event()
            // `Never` can't be constructed, so the `UserEvent` variant can't
            // be present here.
            .unwrap_or_else(|_| unsafe { unreachable_unchecked() })
    }
}

pub trait EventHandler: Debug {
    fn handle_nonuser_event(&mut self, event: Event<Never>, control_flow: *mut ControlFlow);
    //fn handle_user_events(&mut self, control_flow: &mut ControlFlow);
}

struct EventLoopHandler<F, T: 'static> {
    callback: F,
    window_target: RootWindowTarget<T>,
}

impl<F, T: 'static> Debug for EventLoopHandler<F, T> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter.debug_struct("EventLoopHandler")
            .field("window_target", &self.window_target)
            .finish()
    }
}

impl<F, T> EventHandler for EventLoopHandler<F, T>
where
    F: 'static + FnMut(Event<T>, &RootWindowTarget<T>, &mut ControlFlow),
    T: 'static,
{
    fn handle_nonuser_event(&mut self, event: Event<Never>, control_flow: *mut ControlFlow) {
        (self.callback)(
            event.userify(),
            &self.window_target,
            unsafe { &mut *control_flow },
        );
    }

    /*fn handle_user_events(&mut self, control_flow: &mut ControlFlow) {
        for event in self.event_loop.inner.receiver.try_iter() {
            (self.callback)(
                Event::UserEvent(event),
                &self.event_loop,
                control_flow,
            );
        }
    }*/
}

#[derive(Default)]
struct Handler {
    control_flow: ControlFlow,
    control_flow_prev: ControlFlow,
    callback: Option<Box<dyn EventHandler>>,
    waker: EventLoopWaker,
}

unsafe impl Send for Handler {}
unsafe impl Sync for Handler {}

impl Handler {
    fn handle_nonuser_event(&mut self, event: Event<Never>) {
        let control_flow = &mut self.control_flow;
        if let Some(ref mut callback) = self.callback {
            callback.handle_nonuser_event(event, control_flow);
        }
    }
}

pub enum AppState {}

impl AppState {
    fn handler() -> MutexGuard<'static, Handler> {
        HANDLER.lock().unwrap()
    }

    fn events() -> MutexGuard<'static, VecDeque<Event<Never>>> {
        EVENTS.lock().unwrap()
    }

    pub fn set_callback<F, T>(callback: F, window_target: RootWindowTarget<T>)
    where
        F: 'static + FnMut(Event<T>, &RootWindowTarget<T>, &mut ControlFlow),
        T: 'static,
    {
        Self::handler().callback = Some(Box::new(EventLoopHandler {
            callback,
            window_target,
        }));
    }

    pub fn exit() -> ! {
        let mut handler = Self::handler();
        if let Some(mut callback) = handler.callback.take() {
            callback.handle_nonuser_event(
                Event::LoopDestroyed,
                &mut handler.control_flow,
            );
        }
        std::process::exit(0)
    }

    pub fn launched() {
        let mut handler = Self::handler();
        handler.waker.start();
        handler.handle_nonuser_event(Event::NewEvents(StartCause::Init));
    }

    pub fn wakeup() {
        let mut handler = Self::handler();
        handler.control_flow_prev = handler.control_flow;
        let cause = match handler.control_flow {
            ControlFlow::Poll => StartCause::Poll,
            /*ControlFlow::Wait => StartCause::WaitCancelled {
                start,
                requested_resume: None,
            },
            ControlFlow::WaitUntil(requested_resume) => {
                if Instant::now() >= requested_resume {
                    StartCause::ResumeTimeReached {
                        start,
                        requested_resume,
                    }
                } else {
                    StartCause::WaitCancelled {
                        start,
                        requested_resume: Some(requested_resume),
                    }
                }
            },*/
            ControlFlow::Exit => StartCause::Poll,//panic!("unexpected `ControlFlow::Exit`"),
            _ => unimplemented!(),
        };
        handler.handle_nonuser_event(Event::NewEvents(cause));
    }

    pub fn queue_event(event: Event<Never>) {
        Self::events().push_back(event);
    }

    pub fn queue_events(mut events: VecDeque<Event<Never>>) {
        Self::events().append(&mut events);
    }

    pub fn cleared() {
        let mut handler = Self::handler();
        handler.handle_nonuser_event(Event::EventsCleared);
        let events = mem::replace(&mut *Self::events(), Default::default());
        for event in events {
            handler.handle_nonuser_event(event);
        }
        let old = handler.control_flow_prev;
        let new = handler.control_flow;
        match (old, new) {
            (ControlFlow::Poll, ControlFlow::Poll) => (),
            (ControlFlow::Wait, ControlFlow::Wait) => (),
            (ControlFlow::WaitUntil(old_instant), ControlFlow::WaitUntil(new_instant)) if old_instant == new_instant => (),
            (_, ControlFlow::Wait) => handler.waker.stop(),
            (_, ControlFlow::WaitUntil(new_instant)) => handler.waker.start_at(new_instant),
            (_, ControlFlow::Poll) => handler.waker.start(),
            (_, ControlFlow::Exit) => {
                let _: () = unsafe { msg_send![NSApp(), stop:nil] };
            },
        }
    }
}
