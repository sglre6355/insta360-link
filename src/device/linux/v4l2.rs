//! A camera reached through its V4L2 device node, owned by the kernel's
//! `uvcvideo` driver.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use super::sys;
use crate::device::info::DeviceInfo;
use crate::device::transport::{Control, ControlInfo, Transport, XuQuery};
use crate::error::{Error, Result};

/// `V4L2_CID_BASE`, the start of the user control class.
const USER_BASE: u32 = 0x0098_0900;
/// `V4L2_CID_CAMERA_CLASS_BASE`, the start of the camera control class.
const CAMERA_BASE: u32 = 0x009a_0900;

/// `V4L2_CTRL_FLAG_DISABLED`: the driver has disabled the control.
const FLAG_DISABLED: u32 = 0x0001;

/// The `V4L2_CID_*` identifier of a control.
const fn control_id(control: Control) -> u32 {
    match control {
        // V4L2_CID_BRIGHTNESS
        Control::Brightness => USER_BASE,
        // V4L2_CID_CONTRAST
        Control::Contrast => USER_BASE + 1,
        // V4L2_CID_SATURATION
        Control::Saturation => USER_BASE + 2,
        // V4L2_CID_HUE
        Control::Hue => USER_BASE + 3,
        // V4L2_CID_AUTO_WHITE_BALANCE
        Control::AutoWhiteBalance => USER_BASE + 12,
        // V4L2_CID_POWER_LINE_FREQUENCY
        Control::PowerLineFrequency => USER_BASE + 24,
        // V4L2_CID_WHITE_BALANCE_TEMPERATURE
        Control::WhiteBalanceTemperature => USER_BASE + 26,
        // V4L2_CID_SHARPNESS
        Control::Sharpness => USER_BASE + 27,
        // V4L2_CID_PAN_ABSOLUTE
        Control::Pan => CAMERA_BASE + 8,
        // V4L2_CID_TILT_ABSOLUTE
        Control::Tilt => CAMERA_BASE + 9,
        // V4L2_CID_FOCUS_ABSOLUTE
        Control::Focus => CAMERA_BASE + 10,
        // V4L2_CID_FOCUS_AUTO
        Control::AutoFocus => CAMERA_BASE + 12,
        // V4L2_CID_ZOOM_ABSOLUTE
        Control::Zoom => CAMERA_BASE + 13,
    }
}

/// Opens the device node of a camera for control access.
pub(crate) fn open(info: &DeviceInfo) -> Result<Box<dyn Transport>> {
    let device = V4l2Device::open(Path::new(info.id.as_str())).map_err(|err| match err.kind() {
        io::ErrorKind::PermissionDenied => Error::PermissionDenied(info.id.clone()),
        _ => Error::io(format!("cannot open {}", info.id), err),
    })?;
    Ok(Box::new(device))
}

/// An open V4L2 device node.
#[derive(Debug)]
struct V4l2Device {
    file: File,
}

impl V4l2Device {
    /// Opens a `/dev/video*` node for control access.
    ///
    /// This does not start streaming and does not stop other programs from
    /// using the camera.
    fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)?;
        Ok(Self { file })
    }
}

impl Transport for V4l2Device {
    fn xu_query(
        &self,
        unit_id: u8,
        selector: u8,
        query: XuQuery,
        data: &mut [u8],
    ) -> io::Result<()> {
        sys::xu_query(self.file.as_fd(), unit_id, selector, query as u8, data)
    }

    fn query_control(&self, control: Control) -> io::Result<Option<ControlInfo>> {
        let raw = match sys::query_control(self.file.as_fd(), control_id(control)) {
            Ok(raw) => raw,
            // The driver answers EINVAL for a control the device lacks.
            Err(err) if err.kind() == io::ErrorKind::InvalidInput => return Ok(None),
            Err(err) => return Err(err),
        };
        if raw.flags & FLAG_DISABLED != 0 {
            return Ok(None);
        }
        Ok(Some(ControlInfo {
            minimum: raw.minimum,
            maximum: raw.maximum,
            step: raw.step,
            default: raw.default,
        }))
    }

    fn get_controls(&self, controls: &mut [(Control, i32)]) -> io::Result<()> {
        let mut raw: Vec<_> = controls
            .iter()
            .map(|&(control, value)| (control_id(control), value))
            .collect();
        sys::get_controls(self.file.as_fd(), &mut raw)?;
        for (slot, (_, value)) in controls.iter_mut().zip(raw) {
            slot.1 = value;
        }
        Ok(())
    }

    fn set_controls(&self, controls: &[(Control, i32)]) -> io::Result<()> {
        let raw: Vec<_> = controls
            .iter()
            .map(|&(control, value)| (control_id(control), value))
            .collect();
        sys::set_controls(self.file.as_fd(), &raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_ids_match_the_kernel_header() {
        assert_eq!(control_id(Control::Brightness), 0x0098_0900);
        assert_eq!(control_id(Control::Contrast), 0x0098_0901);
        assert_eq!(control_id(Control::Saturation), 0x0098_0902);
        assert_eq!(control_id(Control::Hue), 0x0098_0903);
        assert_eq!(control_id(Control::AutoWhiteBalance), 0x0098_090c);
        assert_eq!(control_id(Control::PowerLineFrequency), 0x0098_0918);
        assert_eq!(control_id(Control::WhiteBalanceTemperature), 0x0098_091a);
        assert_eq!(control_id(Control::Sharpness), 0x0098_091b);
        assert_eq!(control_id(Control::Pan), 0x009a_0908);
        assert_eq!(control_id(Control::Tilt), 0x009a_0909);
        assert_eq!(control_id(Control::Focus), 0x009a_090a);
        assert_eq!(control_id(Control::AutoFocus), 0x009a_090c);
        assert_eq!(control_id(Control::Zoom), 0x009a_090d);
    }
}
