//! Pan and tilt control.
//!
//! The Link series controls its gimbal through standard UVC controls. Pan and
//! tilt share a single control and must therefore be written together.
//!
//! The standard control only reports the last commanded position, ignoring
//! movements caused by AI tracking, gestures, or manual adjustments. When
//! available, a vendor selector is used instead to read the actual gimbal
//! position.

use std::ops::RangeInclusive;

use super::to_raw;
use crate::camera::Camera;
use crate::device::selectors::control;
use crate::device::transport::Control;
use crate::device::transport::ControlInfo;
use crate::error::{Error, Optional, Result};

/// The pan/tilt controls count in arc-seconds.
const UNITS_PER_DEGREE: f64 = 3600.0;

/// A gimbal position in degrees. Zero is the center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanTilt {
    /// Horizontal angle.
    pub pan: f64,
    /// Vertical angle.
    pub tilt: f64,
}

/// A change to the gimbal position. Axes left as `None` keep their current
/// angle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PanTiltUpdate {
    /// New horizontal angle in degrees.
    pub pan: Option<f64>,
    /// New vertical angle in degrees.
    pub tilt: Option<f64>,
}

/// The angles the gimbal can reach, in degrees.
#[derive(Clone, Debug, PartialEq)]
pub struct PanTiltLimits {
    /// Horizontal travel.
    pub pan: RangeInclusive<f64>,
    /// Vertical travel.
    pub tilt: RangeInclusive<f64>,
}

/// Decodes the measured position: two little-endian `i32`s in arc-seconds, tilt
/// first. Returns raw `(pan, tilt)`, or `None` for any other length.
fn decode_measured(data: &[u8]) -> Option<(i32, i32)> {
    let bytes: [u8; 8] = data.try_into().ok()?;
    let word = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
    Some((word(4), word(0)))
}

impl Camera {
    fn pan_tilt_info(&self) -> Result<(ControlInfo, ControlInfo)> {
        Ok((
            self.control_info(Control::Pan)?,
            self.control_info(Control::Tilt)?,
        ))
    }

    /// The range of angles the gimbal accepts.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::Unsupported`] on cameras without a gimbal.
    pub fn position_limits(&self) -> Result<PanTiltLimits> {
        let (pan, tilt) = self.pan_tilt_info()?;
        let degrees = |info: ControlInfo| {
            f64::from(info.minimum) / UNITS_PER_DEGREE..=f64::from(info.maximum) / UNITS_PER_DEGREE
        };
        Ok(PanTiltLimits {
            pan: degrees(pan),
            tilt: degrees(tilt),
        })
    }

    /// The gimbal's current position.
    ///
    /// This is where the gimbal actually points, including moves made by AI
    /// tracking, gestures or by hand, on cameras that report it. Otherwise it
    /// is the last position that was set.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::PositionUnknown`] if the camera reports a value
    /// outside its own range, which some firmware does until the first position
    /// is set.
    pub fn position(&self) -> Result<PanTilt> {
        let (pan_info, tilt_info) = self.pan_tilt_info()?;
        let plausible = |(pan, tilt): (i32, i32)| {
            (pan_info.minimum..=pan_info.maximum).contains(&pan)
                && (tilt_info.minimum..=tilt_info.maximum).contains(&tilt)
        };
        let measured = self
            .xu_read(control::MEASURED_POSITION)
            .optional()?
            .and_then(|data| decode_measured(&data))
            .filter(|&raw| plausible(raw));
        let (pan, tilt) = match measured {
            Some(measured) => measured,
            None => {
                let mut values = [(Control::Pan, 0), (Control::Tilt, 0)];
                self.transport()
                    .get_controls(&mut values)
                    .map_err(|err| Error::io("cannot read pan/tilt", err))?;
                let commanded = (values[0].1, values[1].1);
                if !plausible(commanded) {
                    return Err(Error::PositionUnknown);
                }
                commanded
            }
        };
        Ok(PanTilt {
            pan: f64::from(pan) / UNITS_PER_DEGREE,
            tilt: f64::from(tilt) / UNITS_PER_DEGREE,
        })
    }

    /// Converts a position to the values of the pan and tilt controls, or fails
    /// if the gimbal cannot reach it.
    pub(crate) fn position_raw(&self, pan: f64, tilt: f64) -> Result<(i32, i32)> {
        let (pan_info, tilt_info) = self.pan_tilt_info()?;
        Ok((
            to_raw("pan", pan, UNITS_PER_DEGREE, &pan_info)?,
            to_raw("tilt", tilt, UNITS_PER_DEGREE, &tilt_info)?,
        ))
    }

    /// Moves the gimbal. An axis left as `None` stays where it is.
    ///
    /// Angles are rounded to the gimbal's resolution, which the camera reports
    /// as the step of its pan and tilt controls. While AI tracking is on, the
    /// camera may move away again immediately.
    pub fn set_position(&self, update: PanTiltUpdate) -> Result<()> {
        if update.pan.is_none() && update.tilt.is_none() {
            return Ok(());
        }
        let (pan_info, tilt_info) = self.pan_tilt_info()?;
        let current = match update {
            PanTiltUpdate {
                pan: Some(_),
                tilt: Some(_),
            } => None,
            _ => Some(self.position()?),
        };
        let axis = |what, new: Option<f64>, old: fn(&PanTilt) -> f64, info| {
            let degrees = new
                .or(current.as_ref().map(old))
                .expect("one of them is set");
            to_raw(what, degrees, UNITS_PER_DEGREE, info)
        };
        let pan = axis("pan", update.pan, |p| p.pan, &pan_info)?;
        let tilt = axis("tilt", update.tilt, |p| p.tilt, &tilt_info)?;
        self.transport()
            .set_controls(&[(Control::Pan, pan), (Control::Tilt, tilt)])
            .map_err(|err| Error::io("cannot set pan/tilt", err))
    }

    /// Points the gimbal straight ahead.
    pub fn recenter(&self) -> Result<()> {
        self.set_position(PanTiltUpdate {
            pan: Some(0.0),
            tilt: Some(0.0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::transport::Control;
    use crate::testing::FakeCamera;

    fn gimbal_camera() -> FakeCamera {
        FakeCamera::new()
            .control(Control::Pan, -522_000, 522_000, 3600, 0)
            .control(Control::Tilt, -324_000, 360_000, 3600, 0)
    }

    #[test]
    fn reads_position_in_degrees() {
        let fake = gimbal_camera();
        fake.set(Control::Pan, 36_000);
        fake.set(Control::Tilt, -14_400);
        let position = fake.camera().position().unwrap();
        assert_eq!(
            position,
            PanTilt {
                pan: 10.0,
                tilt: -4.0
            }
        );
    }

    fn measured(pan: i32, tilt: i32) -> Vec<u8> {
        [tilt.to_le_bytes(), pan.to_le_bytes()].concat()
    }

    #[test]
    fn decodes_the_measured_position_tilt_first() {
        let payload = [0xd0, 0x4e, 0xff, 0xff, 0xf8, 0x7f, 0x00, 0x00];
        assert_eq!(decode_measured(&payload), Some((32_760, -45_360)));
        assert_eq!(decode_measured(&payload[..4]), None);
        assert_eq!(decode_measured(&[0; 9]), None);
    }

    #[test]
    fn measured_position_wins_over_the_last_commanded_one() {
        // Captured while AI tracking had moved the gimbal.
        let payload = [0xd0, 0x4e, 0xff, 0xff, 0xf8, 0x7f, 0x00, 0x00];
        let fake = gimbal_camera().selector(control::MEASURED_POSITION, &payload);
        let position = fake.camera().position().unwrap();
        assert_eq!(
            position,
            PanTilt {
                pan: 9.1,
                tilt: -12.6
            }
        );
    }

    #[test]
    fn single_axis_moves_start_from_the_measured_position() {
        let fake = gimbal_camera().selector(control::MEASURED_POSITION, &measured(32_760, -45_360));
        fake.camera()
            .set_position(PanTiltUpdate {
                pan: Some(30.0),
                tilt: None,
            })
            .unwrap();
        // Tilt keeps the measured -12.6 degrees, rounded to the 1 degree step.
        assert_eq!(
            fake.writes().last().unwrap(),
            &[(Control::Pan, 108_000), (Control::Tilt, -46_800)]
        );
    }

    #[test]
    fn implausible_or_malformed_measurements_fall_back() {
        for payload in [measured(36_772_896, 0), vec![0; 4]] {
            let fake = gimbal_camera().selector(control::MEASURED_POSITION, &payload);
            fake.set(Control::Pan, 36_000);
            assert_eq!(fake.camera().position().unwrap().pan, 10.0);
        }
    }

    #[test]
    fn limits_are_in_degrees() {
        let limits = gimbal_camera().camera().position_limits().unwrap();
        assert_eq!(limits.pan, -145.0..=145.0);
        assert_eq!(limits.tilt, -90.0..=100.0);
    }

    #[test]
    fn changing_one_axis_keeps_the_other() {
        let fake = gimbal_camera();
        fake.set(Control::Pan, 36_000);
        fake.set(Control::Tilt, -14_400);
        let camera = fake.camera();
        camera
            .set_position(PanTiltUpdate {
                pan: Some(-20.0),
                tilt: None,
            })
            .unwrap();
        assert_eq!(fake.get(Control::Pan), -72_000);
        assert_eq!(fake.get(Control::Tilt), -14_400);
        // Both axes always travel in one request.
        assert_eq!(
            fake.writes().last().unwrap(),
            &[(Control::Pan, -72_000), (Control::Tilt, -14_400)]
        );
    }

    #[test]
    fn angles_are_rounded_to_the_gimbal_step() {
        let fake = gimbal_camera();
        fake.camera()
            .set_position(PanTiltUpdate {
                pan: Some(10.4),
                tilt: Some(-0.6),
            })
            .unwrap();
        assert_eq!(fake.get(Control::Pan), 36_000);
        assert_eq!(fake.get(Control::Tilt), -3600);
    }

    #[test]
    fn implausible_readback_blocks_single_axis_moves_only() {
        let fake = gimbal_camera();
        fake.set(Control::Pan, 36_772_896);
        fake.set(Control::Tilt, -402_456_063);
        let camera = fake.camera();
        assert!(matches!(camera.position(), Err(Error::PositionUnknown)));
        let pan_only = PanTiltUpdate {
            pan: Some(5.0),
            tilt: None,
        };
        assert!(matches!(
            camera.set_position(pan_only),
            Err(Error::PositionUnknown)
        ));
        assert!(fake.writes().is_empty());
        camera.recenter().unwrap();
        assert_eq!((fake.get(Control::Pan), fake.get(Control::Tilt)), (0, 0));
    }

    #[test]
    fn out_of_range_angles_are_rejected_before_writing() {
        let fake = gimbal_camera();
        let err = fake
            .camera()
            .set_position(PanTiltUpdate {
                pan: Some(146.0),
                tilt: Some(0.0),
            })
            .unwrap_err();
        assert!(matches!(
            err,
            Error::OutOfRange { what: "pan", min, max, .. } if min == -145.0 && max == 145.0
        ));
        assert!(fake.writes().is_empty());
    }

    #[test]
    fn empty_update_does_nothing() {
        let fake = FakeCamera::new();
        fake.camera()
            .set_position(PanTiltUpdate::default())
            .unwrap();
        assert!(fake.writes().is_empty());
    }

    #[test]
    fn cameras_without_a_gimbal_report_unsupported() {
        let fake = FakeCamera::new().control(Control::Zoom, 100, 400, 1, 100);
        let camera = fake.camera();
        assert!(matches!(camera.position(), Err(Error::Unsupported)));
        assert!(matches!(camera.recenter(), Err(Error::Unsupported)));
        assert_eq!(camera.zoom().unwrap(), 1.0);
    }
}
