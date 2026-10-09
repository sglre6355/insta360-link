//! The gimbal related concepts: pan and tilt, zoom, privacy mode and stored
//! presets.

pub(crate) mod position;
pub(crate) mod presets;
mod privacy;
mod zoom;

use crate::device::transport::ControlInfo;
use crate::error::{Error, Result};

/// Converts a value in user units to the nearest step the control accepts.
pub(super) fn to_raw(
    what: &'static str,
    value: f64,
    scale: f64,
    info: &ControlInfo,
) -> Result<i32> {
    let (min, max) = (f64::from(info.minimum), f64::from(info.maximum));
    let raw = value * scale;
    // `contains` is false for NaN, so that is rejected here too.
    if !(min..=max).contains(&raw) {
        return Err(Error::OutOfRange {
            what,
            value,
            min: min / scale,
            max: max / scale,
        });
    }
    let step = f64::from(info.step.max(1));
    Ok((min + ((raw - min) / step).round() * step) as i32)
}
