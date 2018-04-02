use std::env;
use std::fmt;
use std::ptr;
use std::sync::Arc;
use std::os::raw::c_char;
use std::ffi::{CStr, CString};

use super::{ffi, util, XConnection, XError};

unsafe fn open_im(
    xconn: &Arc<XConnection>,
    locale_modifiers: &CStr,
) -> Option<ffi::XIM> {
    // This returns NULL if the locale modifiers string is malformed.
    (xconn.xlib.XSetLocaleModifiers)(locale_modifiers.as_ptr());

    let im = (xconn.xlib.XOpenIM)(
        xconn.display,
        ptr::null_mut(),
        ptr::null_mut(),
        ptr::null_mut(),
    );

    if im.is_null() {
        None
    } else {
        Some(im)
    }
}

#[derive(Debug)]
pub struct InputMethod {
    pub im: ffi::XIM,
    name: String,
}

impl InputMethod {
    fn new(im: ffi::XIM, name: String) -> Self {
        InputMethod { im, name }
    }
}

#[derive(Debug)]
pub enum InputMethodResult {
    /// Input method used locale modifier from XMODIFIERS environment variable.
    XModifiers(InputMethod),
    /// Input method used locale modifier from XIM_SERVERS root window property.
    XimServers(InputMethod),
    /// Input method used internal fallback locale modifier.
    Fallbacks(InputMethod),
    /// Input method could not be opened using any locale modifier tried.
    Failure,
}

impl InputMethodResult {
    pub fn ok(self) -> Option<InputMethod> {
        use self::InputMethodResult::*;
        match self {
            XModifiers(im) | XimServers(im) | Fallbacks(im) => Some(im),
            Failure => None,
        }
    }
}

// The root window has a property named XIM_SERVERS, which contains a list of atoms represeting
// the availabile XIM servers. For instance, if you're using ibus, it would contain an atom named
// "@server=ibus". While it's possible for this property to contain multiple atoms, it's
// presumably rare (though we fortunately handle that anyway, prioritizing the first one).
// Note that we replace "@server=" with "@im=" in order to match the format of locale modifiers.
// Not only because we pass these values to XSetLocaleModifiers, but also because we don't want a
// user who's looking at logs to ask "am I supposed to set XMODIFIERS to `@server=ibus`?!?"
unsafe fn get_xim_servers(xconn: &Arc<XConnection>) -> Result<Vec<String>, XError> {
    let servers_atom = util::get_atom(&xconn, b"XIM_SERVERS\0")?;

    let root = (xconn.xlib.XDefaultRootWindow)(xconn.display);

    let mut atoms: Vec<ffi::Atom> = {
        let result = util::get_property(
            &xconn,
            root,
            servers_atom,
            ffi::XA_ATOM,
        );
        if let Err(util::GetPropertyError::XError(err)) = result {
            return Err(err);
        }
        result.expect("Failed to get XIM_SERVERS root window property")
    };

    let mut names: Vec<*const c_char> = Vec::with_capacity(atoms.len());
    (xconn.xlib.XGetAtomNames)(
        xconn.display,
        atoms.as_mut_ptr(),
        atoms.len() as _,
        names.as_mut_ptr() as _,
    );
    names.set_len(atoms.len());

    let mut formatted_names = Vec::with_capacity(names.len());
    for name in names {
        let string = CStr::from_ptr(name)
            .to_owned()
            .into_string()
            .expect("XIM server name was not valid UTF8");
        (xconn.xlib.XFree)(name as _);
        formatted_names.push(string.replace("@server=", "@im="));
    }
    xconn.check_errors()?;
    Ok(formatted_names)
}

#[derive(Clone)]
struct InputMethodName {
    c_string: CString,
    string: String,
}

impl InputMethodName {
    pub fn from_string(string: String) -> Self {
        let c_string = CString::new(string.clone())
            .expect("String used to construct CString contained null byte");
        InputMethodName {
            c_string,
            string,
        }
    }

    pub fn from_str(string: &str) -> Self {
        let c_string = CString::new(string)
            .expect("String used to construct CString contained null byte");
        InputMethodName {
            c_string,
            string: string.to_owned(),
        }
    }
}

impl fmt::Debug for InputMethodName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.string.fmt(f)
    }
}

#[derive(Debug, Clone)]
struct PotentialInputMethod {
    name: InputMethodName,
    failed: bool,
}

impl PotentialInputMethod {
    pub fn from_string(string: String) -> Self {
        PotentialInputMethod {
            name: InputMethodName::from_string(string),
            failed: false,
        }
    }

    pub fn from_str(string: &str) -> Self {
        PotentialInputMethod {
            name: InputMethodName::from_str(string),
            failed: false,
        }
    }

    pub fn reset(&mut self) {
        self.failed = false;
    }

    pub fn open_im(&mut self, xconn: &Arc<XConnection>) -> Option<InputMethod> {
        let im = unsafe { open_im(xconn, &self.name.c_string) };
        self.failed = im.is_none();
        im.map(|im| InputMethod::new(im, self.name.string.clone()))
    }
}

// This is a really fun struct that manages all known possibilities for locale modifiers, and the
// success of each one when tried. By logging this struct, you get a sequential listing of every
// locale modifier tried, where it came from, and if it succceeded.
#[derive(Debug, Clone)]
pub struct PotentialInputMethods {
    // Our favorite source of locale modifiers is the XMODIFIERS environment variable, so it's the
    // first one we try. On correctly configured systems, that's the end of the story.
    xmodifiers: Option<PotentialInputMethod>,
    // If trying to open an input method with XMODIFIERS didn't work (or if XMODIFIERS wasn't
    // defined), we can ask the X server for a list of available XIM servers. It's likely not
    // guaranteed that the names returned by this are identical to their respective locale
    // modifier names, but it's certainly the convention, and trying this doesn't cost us
    // anything (we'd want to retrieve these values from the server anyway, for logging/diagnostic
    // purposes).
    xim_servers: Option<Vec<PotentialInputMethod>>,
    // If nothing else works, we have some standard options at our disposal that should ostensibly
    // always work. For users who only need compose sequences, this ensures that the program
    // launches without a hitch. For users who need more sophisticated IME features, this is more
    // or less a silent failure. Logging features should be added in the future to allow both
    // audiences to be effectively served.
    fallbacks: [PotentialInputMethod; 2],
}

impl PotentialInputMethods {
    pub fn new(xconn: &Arc<XConnection>) -> Self {
        let xmodifiers = env::var("XMODIFIERS")
            .ok()
            .map(PotentialInputMethod::from_string);
        let xim_servers = unsafe { get_xim_servers(xconn) }
            .ok()
            .map(|servers| {
                let mut potentials = Vec::with_capacity(servers.len());
                for server_name in servers {
                    potentials.push(PotentialInputMethod::from_string(server_name));
                }
                potentials
            });
        PotentialInputMethods {
            // Since passing "" to XSetLocaleModifiers results in it defaulting to the value of
            // XMODIFIERS, it's worth noting what happens if XMODIFIERS is also "". If simply
            // running the program with `XMODIFIERS="" cargo run`, then assuming XMODIFIERS is
            // defined in the profile (or parent environment) then that parent XMODIFIERS is used.
            // If that XMODIFIERS value is also "" (i.e. if you ran `export XMODIFIERS=""`), then
            // XSetLocaleModifiers uses the default local input method. Note that defining
            // XMODIFIERS as "" is different from XMODIFIERS not being defined at all, since in
            // that case, we get `None` and end up skipping ahead to the next method.
            xmodifiers,
            // The XIM_SERVERS property can have surprising values. For instance, when I exited
            // ibus to run fcitx, it retained the value denoting ibus. Even more surprising is
            // that the fcitx input method could only be successfully opened using "@im=ibus".
            // Presumably due to this quirk, it's actually possible to alternate between ibus and
            // fcitx in a running application, as our callbacks detect it as the same input method.
            xim_servers,
            fallbacks: [
                // This is a standard input method that supports compose equences, which should
                // always be available. `@im=none` appears to mean the same thing.
                PotentialInputMethod::from_str("@im=local"),
                // This explicitly specifies to use the implementation-dependent default, though
                // that seems to be equivalent to just using the local input method.
                PotentialInputMethod::from_str("@im="),
            ],
        }
    }

    // This resets the `failed` field of every potential input method, ensuring we have accurate
    // information when this struct is re-used by the destruction/instantiation callbacks.
    fn reset(&mut self) {
        if let Some(ref mut locale) = self.xmodifiers {
            locale.reset();
        }

        if let Some(ref mut locales) = self.xim_servers {
            for locale in locales {
                locale.reset();
            }
        }

        for locale in &mut self.fallbacks {
            locale.reset();
        }
    }

    pub fn open_im(&mut self, xconn: &Arc<XConnection>) -> InputMethodResult {
        use self::InputMethodResult::*;

        self.reset();

        if let Some(ref mut locale) = self.xmodifiers {
            let im = locale.open_im(xconn);
            if let Some(im) = im {
                return XModifiers(im);
            }
        }

        if let Some(ref mut locales) = self.xim_servers {
            for locale in locales {
                let im = locale.open_im(xconn);
                if let Some(im) = im {
                    return XimServers(im);
                }
            }
        }

        for locale in &mut self.fallbacks {
            let im = locale.open_im(xconn);
            if let Some(im) = im {
                return Fallbacks(im);
            }
        }

        Failure
    }
}
