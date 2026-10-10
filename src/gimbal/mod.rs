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
    let last_step = ((max - min) / step).floor();
    let nearest_step = ((raw - min) / step).round().min(last_step);
    Ok((min + nearest_step * step) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_stays_on_a_valid_step_within_the_range() {
        for (minimum, maximum, value, expected) in [
            (0, 100, 100.0, 80),
            (-100, 0, 0.0, -20),
            (10, 30, 30.0, 10),
            (0, 120, 120.0, 120),
        ] {
            let info = ControlInfo {
                minimum,
                maximum,
                step: 40,
                default: minimum,
            };
            assert_eq!(to_raw("test", value, 1.0, &info).unwrap(), expected);
        }
    }
}
