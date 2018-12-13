use cocoa::{appkit, base::id};
use objc::{declare::ClassDecl, runtime::{Class, Object, Sel}};

use platform_impl::platform::util;

pub struct AppClass(pub *const Class);
unsafe impl Send for AppClass {}
unsafe impl Sync for AppClass {}

lazy_static! {
    pub static ref APP_CLASS: AppClass = unsafe {
        let superclass = class!(NSApplication);
        let mut decl = ClassDecl::new("WinitApp", superclass).unwrap();

        decl.add_method(
            sel!(sendEvent:),
            send_event as extern fn(&Object, Sel, id),
        );

        AppClass(decl.register())
    };
}

// Normally, holding Cmd + any key never sends us a `keyUp` event for that key.
// Overriding `sendEvent:` like this fixes that. (https://stackoverflow.com/a/15294196)
// Fun fact: Firefox still has this bug! (https://bugzilla.mozilla.org/show_bug.cgi?id=1299553)
extern fn send_event(this: &Object, _sel: Sel, event: id) {
    unsafe {
        use self::appkit::NSEvent;
        // For posterity, there are some undocumented event types
        // (https://github.com/servo/cocoa-rs/issues/155)
        // but that doesn't really matter here.
        let event_type = event.eventType();
        let modifier_flags = event.modifierFlags();
        if event_type == appkit::NSKeyUp && util::has_flag(
            modifier_flags,
            appkit::NSEventModifierFlags::NSCommandKeyMask,
        ) {
            let key_window: id = msg_send![this, keyWindow];
            let _: () = msg_send![key_window, sendEvent:event];
        } else {
            let superclass = util::superclass(this);
            let _: () = msg_send![super(this, superclass), sendEvent:event];
        }
    }
}
