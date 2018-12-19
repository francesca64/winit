use std::{
    collections::VecDeque, fmt::{self, Debug, Formatter},
    hint::unreachable_unchecked, mem,
    sync::{atomic::{AtomicBool, Ordering}, Mutex, MutexGuard}, time::Instant,
};

use cocoa::{appkit::NSApp, base::nil};

use {
    event::{Event, StartCause},
    event_loop::{ControlFlow, EventLoopWindowTarget as RootWindowTarget},
};
use platform_impl::platform::{observer::EventLoopWaker, util::Never};

lazy_static! {
    static ref HANDLER: Handler = Default::default();
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
    fn handle_nonuser_event(&mut self, event: Event<Never>, control_flow: &mut ControlFlow);
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
    fn handle_nonuser_event(&mut self, event: Event<Never>, control_flow: &mut ControlFlow) {
        (self.callback)(
            event.userify(),
            &self.window_target,
            control_flow,
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
    ready: AtomicBool,
    control_flow: Mutex<ControlFlow>,
    control_flow_prev: Mutex<ControlFlow>,
    start_time: Mutex<Option<Instant>>,
    callback: Mutex<Option<Box<dyn EventHandler>>>,
    pending_events: Mutex<VecDeque<Event<Never>>>,
    waker: Mutex<EventLoopWaker>,
}

unsafe impl Send for Handler {}
unsafe impl Sync for Handler {}

impl Handler {
    fn events<'a>(&'a self) -> MutexGuard<'a, VecDeque<Event<Never>>> {
        self.pending_events.lock().unwrap()
    }

    fn waker<'a>(&'a self) -> MutexGuard<'a, EventLoopWaker> {
        self.waker.lock().unwrap()
    }

    fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    fn set_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }

    fn is_control_flow_exit(&self) -> bool {
        *self.control_flow.lock().unwrap() == ControlFlow::Exit
    }

    fn get_control_flow_and_update_prev(&self) -> ControlFlow {
        let control_flow = self.control_flow.lock().unwrap();
        *self.control_flow_prev.lock().unwrap() = *control_flow;
        *control_flow
    }

    fn get_old_and_new_control_flow(&self) -> (ControlFlow, ControlFlow) {
        let old = *self.control_flow_prev.lock().unwrap();
        let new = *self.control_flow.lock().unwrap();
        (old, new)
    }

    fn get_start_time(&self) -> Option<Instant> {
        *self.start_time.lock().unwrap()
    }

    fn update_start_time(&self) {
        *self.start_time.lock().unwrap() = Some(Instant::now());
    }

    fn take_events(&self) -> VecDeque<Event<Never>> {
        mem::replace(&mut *self.events(), Default::default())
    }

    fn handle_nonuser_event(&self, event: Event<Never>) {
        if let Some(ref mut callback) = *self.callback.lock().unwrap() {
            callback.handle_nonuser_event(
                event,
                &mut *self.control_flow.lock().unwrap(),
            );
        }
    }
}

pub enum AppState {}

impl AppState {
    pub fn set_callback<F, T>(callback: F, window_target: RootWindowTarget<T>)
    where
        F: 'static + FnMut(Event<T>, &RootWindowTarget<T>, &mut ControlFlow),
        T: 'static,
    {
        *HANDLER.callback.lock().unwrap() = Some(Box::new(EventLoopHandler {
            callback,
            window_target,
        }));
    }

    pub fn exit() {
        HANDLER.handle_nonuser_event(Event::LoopDestroyed);
    }

    pub fn launched() {
        HANDLER.set_ready();
        HANDLER.waker().start();
        HANDLER.handle_nonuser_event(Event::NewEvents(StartCause::Init));
    }

    pub fn wakeup() {
        if !HANDLER.is_ready() { return }
        let start = HANDLER.get_start_time().unwrap();
        let cause = match HANDLER.get_control_flow_and_update_prev() {
            ControlFlow::Poll => StartCause::Poll,
            ControlFlow::Wait => StartCause::WaitCancelled {
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
            },
            ControlFlow::Exit => StartCause::Poll,//panic!("unexpected `ControlFlow::Exit`"),
        };
        HANDLER.handle_nonuser_event(Event::NewEvents(cause));
    }

    pub fn queue_event(event: Event<Never>) {
        if !unsafe { msg_send![class!(NSThread), isMainThread] } {
            panic!("uh-oh");
        }
        HANDLER.events().push_back(event);
    }

    pub fn queue_events(mut events: VecDeque<Event<Never>>) {
        if !unsafe { msg_send![class!(NSThread), isMainThread] } {
            panic!("uh-ohs");
        }
        HANDLER.events().append(&mut events);
    }

    pub fn cleared() {
        if !HANDLER.is_ready() { return }
        let mut will_stop = HANDLER.is_control_flow_exit();
        for event in HANDLER.take_events() {
            HANDLER.handle_nonuser_event(event);
            will_stop |= HANDLER.is_control_flow_exit();
        }
        HANDLER.handle_nonuser_event(Event::EventsCleared);
        will_stop |= HANDLER.is_control_flow_exit();
        if will_stop {
            let _: () = unsafe { msg_send![NSApp(), stop:nil] };
            return
        }
        HANDLER.update_start_time();
        match HANDLER.get_old_and_new_control_flow() {
            (ControlFlow::Exit, _) | (_, ControlFlow::Exit) => unreachable!(),
            (old, new) if old == new => (),
            (_, ControlFlow::Wait) => HANDLER.waker().stop(),
            (_, ControlFlow::WaitUntil(instant)) => HANDLER.waker().start_at(instant),
            (_, ControlFlow::Poll) => HANDLER.waker().start(),
        }
    }
}
