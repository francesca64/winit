//! Winit allows you to build a window on as many platforms as possible.
//!
//! # Building a window
//!
//! Before you can build a window, you first need to build an `EventLoop`. This is done with the
//! `EventLoop::new()` function. Example:
//!
//! ```no_run
//! use winit::EventLoop;
//! let events_loop = EventLoop::new();
//! ```
//!
//! Once this is done there are two ways to create a window:
//!
//!  - Calling `Window::new(&events_loop)`.
//!  - Calling `let builder = WindowBuilder::new()` then `builder.build(&events_loop)`.
//!
//! The first way is the simplest way and will give you default values for everything.
//!
//! The second way allows you to customize the way your window will look and behave by modifying
//! the fields of the `WindowBuilder` object before you create the window.
//!
//! # Events handling
//!
//! Once a window has been created, it will *generate events*. For example whenever the user moves
//! the window, resizes the window, moves the mouse, etc. an event is generated.
//!
//! The events generated by a window can be retreived from the `EventLoop` the window was created
//! with.
//!
//! You do this by calling `events_loop.run(...)`. This function will run forever unless it is
//! stopped by returning `ControlFlow::Exit`, at which point the entire program will terminate.
//!
//! ```no_run
//! use winit::{ControlFlow, Event, WindowEvent};
//! # use winit::EventLoop;
//! # let events_loop = EventLoop::new();
//!
//! events_loop.run(move |event, _, control_flow| {
//!     match event {
//!         Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
//!             println!("The close button was pressed; stopping");
//!             *control_flow = ControlFlow::Exit
//!         },
//!         _ => *control_flow = ControlFlow::Wait,
//!     }
//! });
//! ```
//!
//! If you use multiple windows, the `WindowEvent` event has a member named `window_id`. You can
//! compare it with the value returned by the `id()` method of `Window` in order to know which
//! window has received the event.
//!
//! # Drawing on the window
//!
//! Winit doesn't provide any function that allows drawing on a window. However it allows you to
//! retrieve the raw handle of the window (see the `os` module for that), which in turn allows you
//! to create an OpenGL/Vulkan/DirectX/Metal/etc. context that will draw on the window.
//!

#[allow(unused_imports)]
#[macro_use]
extern crate lazy_static;
extern crate libc;
#[macro_use]
extern crate log;
#[cfg(feature = "icon_loading")]
extern crate image;
#[cfg(feature = "serde")]
#[macro_use]
extern crate serde;

#[cfg(target_os = "windows")]
extern crate winapi;
#[cfg(target_os = "windows")]
#[macro_use]
extern crate crossbeam_channel;
#[cfg(any(target_os = "macos", target_os = "ios"))]
#[macro_use]
extern crate objc;
#[cfg(target_os = "macos")]
extern crate cocoa;
#[cfg(target_os = "macos")]
extern crate core_foundation;
#[cfg(target_os = "macos")]
extern crate core_graphics;
#[cfg(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))]
extern crate x11_dl;
#[cfg(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "windows"))]
extern crate parking_lot;
#[cfg(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))]
extern crate percent_encoding;
#[cfg(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))]
extern crate smithay_client_toolkit as sctk;

use std::time::Instant;
pub(crate) use dpi::*; // TODO: Actually change the imports throughout the codebase.
pub use events::*;
pub use window::{AvailableMonitorsIter, MonitorId};
pub use icon::*;

pub mod dpi;
mod events;
mod icon;
mod platform_impl;
mod window;

pub mod os;

/// Represents a window.
///
/// # Example
///
/// ```no_run
/// use winit::{Event, EventLoop, Window, WindowEvent, ControlFlow};
///
/// let mut events_loop = EventLoop::new();
/// let window = Window::new(&events_loop).unwrap();
///
/// events_loop.run(move |event, _, control_flow| {
///     match event {
///         Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
///             *control_flow = ControlFlow::Exit
///         },
///         _ => *control_flow = ControlFlow::Wait,
///     }
/// });
/// ```
pub struct Window {
    window: platform_impl::Window,
}

impl std::fmt::Debug for Window {
    fn fmt(&self, fmtr: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmtr.pad("Window { .. }")
    }
}

/// Identifier of a window. Unique for each window.
///
/// Can be obtained with `window.id()`.
///
/// Whenever you receive an event specific to a window, this event contains a `WindowId` which you
/// can then compare to the ids of your windows.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(platform_impl::WindowId);

/// Identifier of an input device.
///
/// Whenever you receive an event arising from a particular input device, this event contains a `DeviceId` which
/// identifies its origin. Note that devices may be virtual (representing an on-screen cursor and keyboard focus) or
/// physical. Virtual devices typically aggregate inputs from multiple physical devices.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(platform_impl::DeviceId);

/// Provides a way to retrieve events from the system and from the windows that were registered to
/// the events loop.
///
/// An `EventLoop` can be seen more or less as a "context". Calling `EventLoop::new()`
/// initializes everything that will be required to create windows. For example on Linux creating
/// an events loop opens a connection to the X or Wayland server.
///
/// To wake up an `EventLoop` from a another thread, see the `EventLoopProxy` docs.
///
/// Note that the `EventLoop` cannot be shared accross threads (due to platform-dependant logic
/// forbiding it), as such it is neither `Send` nor `Sync`. If you need cross-thread access, the
/// `Window` created from this `EventLoop` _can_ be sent to an other thread, and the
/// `EventLoopProxy` allows you to wakeup an `EventLoop` from an other thread.
pub struct EventLoop<T> {
    events_loop: platform_impl::EventLoop<T>,
    _marker: ::std::marker::PhantomData<*mut ()> // Not Send nor Sync
}

impl<T> std::fmt::Debug for EventLoop<T> {
    fn fmt(&self, fmtr: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmtr.pad("EventLoop { .. }")
    }
}

/// Returned by the user callback given to the `EventLoop::run_forever` method.
///
/// Indicates whether the `run_forever` method should continue or complete.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ControlFlow {
    /// When the current loop iteration finishes, suspend the thread until another event arrives.
    Wait,
    /// When the current loop iteration finishes, suspend the thread until either another event
    /// arrives or the given time is reached.
    WaitUntil(Instant),
    /// When the current loop iteration finishes, immediately begin a new iteration regardless of
    /// whether or not new events are available to process.
    Poll,
    /// Send a `LoopDestroyed` event and stop the event loop.
    Exit
}

impl Default for ControlFlow {
    #[inline(always)]
    fn default() -> ControlFlow {
        ControlFlow::Poll
    }
}

impl EventLoop<()> {
    pub fn new() -> EventLoop<()> {
        EventLoop::<()>::new_user_event()
    }
}

impl<T> EventLoop<T> {
    /// Builds a new events loop.
    ///
    /// Usage will result in display backend initialisation, this can be controlled on linux
    /// using an environment variable `WINIT_UNIX_BACKEND`. Legal values are `x11` and `wayland`.
    /// If it is not set, winit will try to connect to a wayland connection, and if it fails will
    /// fallback on x11. If this variable is set with any other value, winit will panic.
    pub fn new_user_event() -> EventLoop<T> {
        EventLoop {
            events_loop: platform_impl::EventLoop::new(),
            _marker: ::std::marker::PhantomData,
        }
    }

    /// Returns the list of all the monitors available on the system.
    ///
    // Note: should be replaced with `-> impl Iterator` once stable.
    #[inline]
    pub fn get_available_monitors(&self) -> AvailableMonitorsIter {
        let data = self.events_loop.get_available_monitors();
        AvailableMonitorsIter{ data: data.into_iter() }
    }

    /// Returns the primary monitor of the system.
    #[inline]
    pub fn get_primary_monitor(&self) -> MonitorId {
        MonitorId { inner: self.events_loop.get_primary_monitor() }
    }

    /// Hijacks the calling thread and initializes the `winit` event loop. Can take a
    /// `FnMut(Event, &EventLoop) -> ControlFlow` or a custom `EventHandler` type.
    ///
    /// Any values not passed to this function will *not* be dropped.
    #[inline]
    pub fn run<F>(self, event_handler: F) -> !
        where F: 'static + FnMut(Event<T>, &EventLoop<T>, &mut ControlFlow)
    {
        self.events_loop.run(event_handler)
    }

    /// Creates an `EventLoopProxy` that can be used to wake up the `EventLoop` from another
    /// thread.
    pub fn create_proxy(&self) -> EventLoopProxy<T> {
        EventLoopProxy {
            events_loop_proxy: self.events_loop.create_proxy(),
        }
    }
}

/// Used to wake up the `EventLoop` from another thread.
#[derive(Clone)]
pub struct EventLoopProxy<T> {
    events_loop_proxy: platform_impl::EventLoopProxy<T>,
}

impl<T> std::fmt::Debug for EventLoopProxy<T> {
    fn fmt(&self, fmtr: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmtr.pad("EventLoopProxy { .. }")
    }
}

impl<T> EventLoopProxy<T> {
    /// Send an event to the `EventLoop` from which this proxy was created. This emits a
    /// `UserEvent(event)` event in the event loop, where `event` is the value passed to this
    /// function.
    ///
    /// Returns an `Err` if the associated `EventLoop` no longer exists.
    pub fn send_event(&self, event: T) -> Result<(), EventLoopClosed> {
        self.events_loop_proxy.send_event(event)
    }
}

/// The error that is returned when an `EventLoopProxy` attempts to wake up an `EventLoop` that
/// no longer exists.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct EventLoopClosed;

impl std::fmt::Display for EventLoopClosed {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", std::error::Error::description(self))
    }
}

impl std::error::Error for EventLoopClosed {
    fn description(&self) -> &str {
        "Tried to wake up a closed `EventLoop`"
    }
}

/// Object that allows you to build windows.
#[derive(Clone)]
pub struct WindowBuilder {
    /// The attributes to use to create the window.
    pub window: WindowAttributes,

    // Platform-specific configuration. Private.
    platform_specific: platform_impl::PlatformSpecificWindowBuilderAttributes,
}

impl std::fmt::Debug for WindowBuilder {
    fn fmt(&self, fmtr: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmtr.debug_struct("WindowBuilder")
            .field("window", &self.window)
            .finish()
    }
}

/// Error that can happen while creating a window or a headless renderer.
#[derive(Debug, Clone)]
pub enum CreationError {
    OsError(String),
    /// TODO: remove this error
    NotSupported,
}

impl CreationError {
    fn to_string(&self) -> &str {
        match *self {
            CreationError::OsError(ref text) => &text,
            CreationError::NotSupported => "Some of the requested attributes are not supported",
        }
    }
}

impl std::fmt::Display for CreationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        formatter.write_str(self.to_string())
    }
}

impl std::error::Error for CreationError {
    fn description(&self) -> &str {
        self.to_string()
    }
}

/// Describes the appearance of the mouse cursor.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MouseCursor {
    /// The platform-dependent default cursor.
    Default,
    /// A simple crosshair.
    Crosshair,
    /// A hand (often used to indicate links in web browsers).
    Hand,
    /// Self explanatory.
    Arrow,
    /// Indicates something is to be moved.
    Move,
    /// Indicates text that may be selected or edited.
    Text,
    /// Program busy indicator.
    Wait,
    /// Help indicator (often rendered as a "?")
    Help,
    /// Progress indicator. Shows that processing is being done. But in contrast
    /// with "Wait" the user may still interact with the program. Often rendered
    /// as a spinning beach ball, or an arrow with a watch or hourglass.
    Progress,

    /// Cursor showing that something cannot be done.
    NotAllowed,
    ContextMenu,
    Cell,
    VerticalText,
    Alias,
    Copy,
    NoDrop,
    Grab,
    Grabbing,
    AllScroll,
    ZoomIn,
    ZoomOut,

    /// Indicate that some edge is to be moved. For example, the 'SeResize' cursor
    /// is used when the movement starts from the south-east corner of the box.
    EResize,
    NResize,
    NeResize,
    NwResize,
    SResize,
    SeResize,
    SwResize,
    WResize,
    EwResize,
    NsResize,
    NeswResize,
    NwseResize,
    ColResize,
    RowResize,
}

impl Default for MouseCursor {
    fn default() -> Self {
        MouseCursor::Default
    }
}

/// Attributes to use when creating a window.
#[derive(Debug, Clone)]
pub struct WindowAttributes {
    /// The dimensions of the window. If this is `None`, some platform-specific dimensions will be
    /// used.
    ///
    /// The default is `None`.
    pub dimensions: Option<LogicalSize>,

    /// The minimum dimensions a window can be, If this is `None`, the window will have no minimum dimensions (aside from reserved).
    ///
    /// The default is `None`.
    pub min_dimensions: Option<LogicalSize>,

    /// The maximum dimensions a window can be, If this is `None`, the maximum will have no maximum or will be set to the primary monitor's dimensions by the platform.
    ///
    /// The default is `None`.
    pub max_dimensions: Option<LogicalSize>,

    /// Whether the window is resizable or not.
    ///
    /// The default is `true`.
    pub resizable: bool,

    /// Whether the window should be set as fullscreen upon creation.
    ///
    /// The default is `None`.
    pub fullscreen: Option<MonitorId>,

    /// The title of the window in the title bar.
    ///
    /// The default is `"winit window"`.
    pub title: String,

    /// Whether the window should be maximized upon creation.
    ///
    /// The default is `false`.
    pub maximized: bool,

    /// Whether the window should be immediately visible upon creation.
    ///
    /// The default is `true`.
    pub visible: bool,

    /// Whether the the window should be transparent. If this is true, writing colors
    /// with alpha values different than `1.0` will produce a transparent window.
    ///
    /// The default is `false`.
    pub transparent: bool,

    /// Whether the window should have borders and bars.
    ///
    /// The default is `true`.
    pub decorations: bool,

    /// Whether the window should always be on top of other windows.
    ///
    /// The default is `false`.
    pub always_on_top: bool,

    /// The window icon.
    ///
    /// The default is `None`.
    pub window_icon: Option<Icon>,

    /// [iOS only] Enable multitouch,
    /// see [multipleTouchEnabled](https://developer.apple.com/documentation/uikit/uiview/1622519-multipletouchenabled)
    pub multitouch: bool,
}

impl Default for WindowAttributes {
    #[inline]
    fn default() -> WindowAttributes {
        WindowAttributes {
            dimensions: None,
            min_dimensions: None,
            max_dimensions: None,
            resizable: true,
            title: "winit window".to_owned(),
            maximized: false,
            fullscreen: None,
            visible: true,
            transparent: false,
            decorations: true,
            always_on_top: false,
            window_icon: None,
            multitouch: false,
        }
    }
}
