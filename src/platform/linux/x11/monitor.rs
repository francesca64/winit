use std::slice;
use std::sync::Arc;

use super::ffi::{
    RROutput,
    XRRCrtcInfo,
    XRRMonitorInfo,
    XRRScreenResources,
};
use super::XConnection;

// Used to test XRandR < 1.5 code path. This should always be committed as false.
const FORCE_RANDR_COMPAT: bool = true;

#[derive(Debug, Clone)]
pub struct MonitorId {
    /// The actual id
    id: u32,
    /// The name of the monitor
    name: String,
    /// The size of the monitor
    dimensions: (u32, u32),
    /// The position of the monitor in the X screen
    position: (i32, i32),
    /// If the monitor is the primary one
    primary: bool,
    /// The DPI scaling factor
    hidpi_factor: f32,
}

impl MonitorId {
    // Keep this private!
    fn from_repr(
        xconn: &Arc<XConnection>,
        resources: *mut XRRScreenResources,
        id: u32,
        repr: MonitorRepr,
        primary: bool,
    ) -> Self {
        unsafe {
            let (name, hidpi_factor) = get_output_info(xconn, resources, &repr);
            MonitorId {
                id,
                name,
                hidpi_factor,
                dimensions: repr.get_dimensions(),
                position: repr.get_position(),
                primary,
            }
        }
    }

    pub fn get_name(&self) -> Option<String> {
        Some(self.name.clone())
    }

    #[inline]
    pub fn get_native_identifier(&self) -> u32 {
        self.id as u32
    }

    pub fn get_dimensions(&self) -> (u32, u32) {
        self.dimensions
    }

    pub fn get_position(&self) -> (i32, i32) {
        self.position
    }

    #[inline]
    pub fn get_hidpi_factor(&self) -> f32 {
        self.hidpi_factor
    }
}

enum MonitorRepr {
    Monitor(*mut XRRMonitorInfo),
    Crtc(*mut XRRCrtcInfo),
}

impl MonitorRepr {
    unsafe fn get_output(&self) -> RROutput {
        match *self {
            // Same member names, but different locations..
            MonitorRepr::Monitor(monitor) => *((*monitor).outputs.offset(0)),
            MonitorRepr::Crtc(crtc) => *((*crtc).outputs.offset(0)),
        }
    }

    unsafe fn get_dimensions(&self) -> (u32, u32) {
        match *self {
            MonitorRepr::Monitor(monitor) => ((*monitor).width as u32, (*monitor).height as u32),
            MonitorRepr::Crtc(crtc) => ((*crtc).width as u32, (*crtc).height as u32),
        }
    }

    unsafe fn get_position(&self) -> (i32, i32) {
        match *self {
            MonitorRepr::Monitor(monitor) => ((*monitor).x as i32, (*monitor).y as i32),
            MonitorRepr::Crtc(crtc) => ((*crtc).x as i32, (*crtc).y as i32),
        }
    }
}

impl From<*mut XRRMonitorInfo> for MonitorRepr {
    fn from(monitor: *mut XRRMonitorInfo) -> Self {
        MonitorRepr::Monitor(monitor)
    }
}

impl From<*mut XRRCrtcInfo> for MonitorRepr {
    fn from(crtc: *mut XRRCrtcInfo) -> Self {
        MonitorRepr::Crtc(crtc)
    }
}

unsafe fn get_output_info(
    xconn: &Arc<XConnection>,
    resources: *mut XRRScreenResources,
    repr: &MonitorRepr,
) -> (String, f32) {
    let output_info = (xconn.xrandr.XRRGetOutputInfo)(
        xconn.display,
        resources,
        repr.get_output(),
    );
    let nameslice = slice::from_raw_parts(
        (*output_info).name as *mut u8,
        (*output_info).nameLen as usize,
    );
    let name = String::from_utf8_lossy(nameslice).into();
    let hidpi_factor = {
        let (width, height) = repr.get_dimensions();
        let x_mm = (*output_info).mm_width as f32;
        let y_mm = (*output_info).mm_height as f32;
        let x_px = width as f32;
        let y_px = height as f32;
        let ppmm = ((x_px * y_px) / (x_mm * y_mm)).sqrt();
        // Quantize 1/12 step size
        ((ppmm * (12.0 * 25.4 / 96.0)).round() / 12.0).max(1.0)
    };
    (xconn.xrandr.XRRFreeOutputInfo)(output_info);
    (name, hidpi_factor)
}

pub fn get_available_monitors(xconn: &Arc<XConnection>) -> Vec<MonitorId> {
    let mut available = Vec::new();
    unsafe {
        let root = (xconn.xlib.XDefaultRootWindow)(xconn.display);
        // WARNING: this function is supposedly very slow, on the order of hundreds of ms.
        let resources = (xconn.xrandr.XRRGetScreenResources)(xconn.display, root);

        if xconn.xrandr_1_5.is_some() && !FORCE_RANDR_COMPAT {
            // We're in XRandR >= 1.5, enumerate Monitors to handle things like MST and videowalls
            let xrandr_1_5 = xconn.xrandr_1_5.as_ref().unwrap();
            let mut monitor_count = 0;
            let monitors = (xrandr_1_5.XRRGetMonitors)(xconn.display, root, 1, &mut monitor_count);
            for monitor_index in 0..monitor_count {
                let monitor = monitors.offset(monitor_index as isize);
                let is_primary = (*monitor).primary != 0;
                available.push(MonitorId::from_repr(
                    xconn,
                    resources,
                    monitor_index as u32,
                    monitor.into(),
                    is_primary,
                ));
            }
            (xrandr_1_5.XRRFreeMonitors)(monitors);
        } else {
            // We're in XRandR < 1.5, enumerate CRTCs. Everything will work but MST and
            // videowall setups will show more monitors than the logical groups the user
            // cares about
            let primary = (xconn.xrandr.XRRGetOutputPrimary)(xconn.display, root);
            println!("PRIMARY {:?}", primary);
            println!("CRTC_COUNT {:?}", (*resources).ncrtc);
            for crtc_index in 0..(*resources).ncrtc {
                println!("> CRTC #{}", crtc_index);
                let crtc_id = *((*resources).crtcs.offset(crtc_index as isize));
                println!("·> ID {}", crtc_id);
                let crtc = (xconn.xrandr.XRRGetCrtcInfo)(xconn.display, resources, crtc_id);
                println!("·> WIDTH {}", (*crtc).width);
                println!("·> HEIGHT {}", (*crtc).height);
                println!("·> OUTPUT_COUNT {}", (*crtc).noutput);
                let is_valid = (*crtc).width > 0 && (*crtc).height > 0 && (*crtc).noutput > 0;
                println!("·> IS_VALID {}", is_valid);
                if is_valid {
                    let crtc = MonitorRepr::from(crtc);
                    let is_primary = crtc.get_output() == primary;
                    available.push(MonitorId::from_repr(
                        xconn,
                        resources,
                        crtc_id as u32,
                        crtc,
                        is_primary,
                    ));
                }
                (xconn.xrandr.XRRFreeCrtcInfo)(crtc);
            }
        }
        (xconn.xrandr.XRRFreeScreenResources)(resources);
    }
    available
}

#[inline]
pub fn get_primary_monitor(x: &Arc<XConnection>) -> MonitorId {
    get_available_monitors(x)
        .into_iter()
        .find(|m| m.primary)
        // 'no primary' case is better handled picking some existing monitor
        .or_else(|| get_available_monitors(x).into_iter().next())
        .expect("[winit] Failed to find any x11 monitor")
}
