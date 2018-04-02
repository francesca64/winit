use std::env;
use std::fmt;
use std::sync::Arc;
use std::os::raw::c_char;
use std::ffi::{CStr, CString};

use super::{ffi, util, XConnection, XError, open_im};

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
    XModifiers(InputMethod),
    XimServers(InputMethod),
    Fallbacks(InputMethod),
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
        let string = CStr::from_ptr(name).to_owned().into_string()
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

#[derive(Debug, Clone)]
pub struct PotentialInputMethods {
    xmodifiers: Option<PotentialInputMethod>,
    xim_servers: Option<Vec<PotentialInputMethod>>,
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
            // Running the program with `XMODIFIERS="" cargo run` will result in...
            // A) if XMODIFIERS is still defined in the environment, then the implementation will
            //    pull that in when we call XSetLocaleModifiers with an empty string.
            // B) if XMODIFIERS is also empty in the profile, or the user did something like
            //    `export XMODIFIERS=""`, then we'll end up using the local input method.
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
