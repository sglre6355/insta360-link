//! Transport interface between [`Camera`](crate::Camera) and the device.
//!
//! Each platform implements [`Transport`] to communicate with the camera. The
//! same interface allows protocol logic to be tested against a fake camera
//! without depending on a particular operating system.

use std::fmt;
use std::io;

/// A UVC request code, as sent to an extension unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum XuQuery {
    /// Writes the current value.
    SetCur = 0x01,
    /// Reads the current value.
    GetCur = 0x81,
    /// Reads the minimum value.
    GetMin = 0x82,
    /// Reads the maximum value.
    GetMax = 0x83,
    /// Reads the resolution (step).
    GetRes = 0x84,
    /// Reads the payload length, as a 2-byte little-endian integer.
    GetLen = 0x85,
    /// Reads the capability flags, as 1 byte.
    GetInfo = 0x86,
    /// Reads the default value.
    GetDef = 0x87,
}

/// A standard UVC control, one that every operating system knows how to reach.
/// Values are in the units UVC defines for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Control {
    /// Image brightness.
    Brightness,
    /// Image contrast.
    Contrast,
    /// Color saturation.
    Saturation,
    /// Hue.
    Hue,
    /// Sharpness.
    Sharpness,
    /// Whether white balance is automatic: 1 or 0.
    AutoWhiteBalance,
    /// White balance temperature in kelvin.
    WhiteBalanceTemperature,
    /// Compensation for mains flicker: 0 off, 1 for 50 Hz, 2 for 60 Hz, 3
    /// automatic.
    PowerLineFrequency,
    /// Pan in arc-seconds. Pan and tilt travel in one request.
    Pan,
    /// Tilt in arc-seconds.
    Tilt,
    /// Zoom in hundredths.
    Zoom,
    /// Focus position.
    Focus,
    /// Whether focus is automatic: 1 or 0.
    AutoFocus,
}

impl Control {
    fn name(self) -> &'static str {
        match self {
            Self::Brightness => "brightness",
            Self::Contrast => "contrast",
            Self::Saturation => "saturation",
            Self::Hue => "hue",
            Self::Sharpness => "sharpness",
            Self::AutoWhiteBalance => "automatic white balance",
            Self::WhiteBalanceTemperature => "white balance temperature",
            Self::PowerLineFrequency => "power line frequency",
            Self::Pan => "pan",
            Self::Tilt => "tilt",
            Self::Zoom => "zoom",
            Self::Focus => "focus",
            Self::AutoFocus => "automatic focus",
        }
    }
}

impl fmt::Display for Control {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The range of a standard control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlInfo {
    /// Smallest accepted value.
    pub minimum: i32,
    /// Largest accepted value.
    pub maximum: i32,
    /// Distance between accepted values.
    pub step: i32,
    /// Factory default.
    pub default: i32,
}

/// Low-level access to one camera.
///
/// Errors are plain [`io::Error`]s from the platform. [`Camera`](crate::Camera)
/// turns them into [`Error`](crate::Error)s.
///
/// A transport must be [`Send`] so that the camera can move to another thread.
pub trait Transport: Send {
    /// Sends one request to the extension unit with the given unit id.
    ///
    /// For [`XuQuery::SetCur`] `data` is the payload to write. For every other
    /// query it receives the answer. Its length must be exactly what the device
    /// expects for that selector.
    fn xu_query(
        &self,
        unit_id: u8,
        selector: u8,
        query: XuQuery,
        data: &mut [u8],
    ) -> io::Result<()>;

    /// Describes a standard control, or returns `None` if the device does not
    /// have it.
    fn query_control(&self, control: Control) -> io::Result<Option<ControlInfo>>;

    /// Reads the controls named by the first element of each pair into the
    /// second.
    fn get_controls(&self, controls: &mut [(Control, i32)]) -> io::Result<()>;

    /// Writes all `(control, value)` pairs in a single request.
    fn set_controls(&self, controls: &[(Control, i32)]) -> io::Result<()>;
}
