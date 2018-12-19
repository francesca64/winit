#![allow(non_camel_case_types)]

use std::os::raw::c_void;

#[repr(C)]
pub struct dispatch_object_s { _private: [u8; 0] }

pub type dispatch_function_t = extern fn(*mut c_void);
pub type dispatch_queue_t = *mut dispatch_object_s;

pub fn dispatch_get_main_queue() -> dispatch_queue_t {
    unsafe { &_dispatch_main_q as *const _ as dispatch_queue_t }
}

#[link(name = "System", kind = "dylib")]
extern {
    static _dispatch_main_q: dispatch_object_s;

    pub fn dispatch_async_f(
        queue: dispatch_queue_t,
        context: *mut c_void,
        work: Option<dispatch_function_t>,
    );
    pub fn dispatch_sync_f(
        queue: dispatch_queue_t,
        context: *mut c_void,
        work: Option<dispatch_function_t>,
    );
}
