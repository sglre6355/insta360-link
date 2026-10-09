//! Exposure: automatic with compensation, or manual ISO and shutter speed.
//!
//! The Link series uses a vendor control unit rather than standard UVC controls
//! for exposure.

use crate::camera::Camera;
use crate::device::selectors::{Selector, control};
use crate::error::{Error, Result};

const AE_MANUAL: u8 = 1;
const AE_AUTO: u8 = 2;
const COMPENSATION_SCALE: f64 = 100.0;
/// Maximum exposure compensation in stops, in either direction. The camera
/// stops responding around -5 and +4 and ignores values far beyond that range.
const COMPENSATION_LIMIT: f64 = 5.0;

/// Who chooses the exposure, and the values that apply in that mode.
///
/// This type is used for both reading and writing. Setting it switches the
/// camera to that mode.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(tag = "mode", rename_all = "kebab-case", deny_unknown_fields)
)]
pub enum Exposure {
    /// The camera adapts to the scene.
    Auto {
        /// Exposure compensation in EV stops: positive is brighter, zero is
        /// neutral.
        compensation: f64,
    },
    /// ISO and shutter stay at fixed values.
    Manual {
        /// Sensor sensitivity. Automatic exposure uses roughly 100 to 3200.
        iso: u16,
        /// Shutter time as the denominator of a fraction of a second: `50` is
        /// 1/50 s.
        shutter: u16,
    },
}

/// Whether the camera's reading matches what was written. The camera rounds
/// some values by one, such as 1/60 s to 1/59 s.
fn close_enough(read: u16, written: u16) -> bool {
    read.abs_diff(written) <= 1
}

impl Camera {
    /// Whether the exposure is manual.
    fn exposure_is_manual(&self) -> Result<bool> {
        let payload = self.xu_read(control::EXPOSURE_MODE)?;
        match payload.first() {
            Some(&AE_AUTO) => Ok(false),
            Some(&AE_MANUAL) => Ok(true),
            _ => Err(Error::Protocol(format!(
                "unrecognized exposure mode bytes {payload:02x?}"
            ))),
        }
    }

    fn exposure_word(&self, selector: Selector) -> Result<[u8; 2]> {
        let payload = self.xu_read(selector)?;
        <[u8; 2]>::try_from(payload.as_slice())
            .map_err(|_| Error::Protocol(format!("{selector} is {} bytes, not 2", payload.len())))
    }

    fn read_u16(&self, selector: Selector) -> Result<u16> {
        self.exposure_word(selector).map(u16::from_le_bytes)
    }

    fn compensation_raw(&self) -> Result<i16> {
        self.exposure_word(control::EXPOSURE_COMPENSATION)
            .map(i16::from_le_bytes)
    }

    /// Reads the exposure mode and the values that apply in it.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::Unsupported`] on a camera that does not report its
    /// exposure.
    pub fn exposure(&self) -> Result<Exposure> {
        if self.exposure_is_manual()? {
            let (iso, shutter) = self.exposure_in_effect()?;
            Ok(Exposure::Manual { iso, shutter })
        } else {
            Ok(Exposure::Auto {
                compensation: f64::from(self.compensation_raw()?) / COMPENSATION_SCALE,
            })
        }
    }

    /// The ISO and shutter the camera is using right now, as `(iso, shutter
    /// denominator)`. In automatic mode these follow the scene.
    ///
    /// Pass these to [`Exposure::Manual`] to switch to manual exposure without
    /// changing the picture. The values the camera would otherwise start from
    /// can be far off.
    pub fn exposure_in_effect(&self) -> Result<(u16, u16)> {
        Ok((
            self.read_u16(control::ISO)?,
            self.read_u16(control::SHUTTER)?,
        ))
    }

    /// Switches to the exposure mode of `exposure`, sets its values, and waits
    /// for the camera to confirm the changes.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::NotStreaming`] if something has to change and no
    /// video is streaming.
    pub fn set_exposure(&self, exposure: &Exposure) -> Result<()> {
        match *exposure {
            Exposure::Auto { compensation } => {
                if !(-COMPENSATION_LIMIT..=COMPENSATION_LIMIT).contains(&compensation) {
                    return Err(Error::OutOfRange {
                        what: "exposure compensation",
                        value: compensation,
                        min: -COMPENSATION_LIMIT,
                        max: COMPENSATION_LIMIT,
                    });
                }
                let raw = (compensation * COMPENSATION_SCALE).round() as i16;
                self.set_exposure_mode(false)?;
                if self.compensation_raw()? != raw {
                    self.require_streaming()?;
                    self.write_verified(
                        control::EXPOSURE_COMPENSATION,
                        &raw.to_le_bytes(),
                        || Ok(self.compensation_raw()? == raw),
                    )?;
                }
            }
            Exposure::Manual { iso, shutter } => {
                for (what, value) in [("ISO", iso), ("shutter", shutter)] {
                    if value == 0 {
                        return Err(Error::OutOfRange {
                            what,
                            value: 0.0,
                            min: 1.0,
                            max: f64::from(u16::MAX),
                        });
                    }
                }
                self.set_exposure_mode(true)?;
                for (selector, value) in [(control::ISO, iso), (control::SHUTTER, shutter)] {
                    if close_enough(self.read_u16(selector)?, value) {
                        continue;
                    }
                    self.require_streaming()?;
                    self.write_verified(selector, &value.to_le_bytes(), || {
                        Ok(close_enough(self.read_u16(selector)?, value))
                    })?;
                }
            }
        }
        Ok(())
    }

    /// Switches between automatic and manual exposure if the camera is not
    /// already in the wanted mode.
    fn set_exposure_mode(&self, manual: bool) -> Result<()> {
        if self.exposure_is_manual()? == manual {
            return Ok(());
        }
        self.require_streaming()?;
        let code = if manual { AE_MANUAL } else { AE_AUTO };
        self.write_verified(control::EXPOSURE_MODE, &[code], || {
            Ok(self.exposure_is_manual()? == manual)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeCamera;

    fn auto(compensation: f64) -> Exposure {
        Exposure::Auto { compensation }
    }

    fn manual(iso: u16, shutter: u16) -> Exposure {
        Exposure::Manual { iso, shutter }
    }

    /// A camera in automatic exposure, currently at ISO 300 and 1/50 s.
    fn exposure_camera() -> FakeCamera {
        FakeCamera::new()
            .selector(control::EXPOSURE_COMPENSATION, &[0, 0])
            .selector(control::ISO, &300u16.to_le_bytes())
            .selector(control::SHUTTER, &50u16.to_le_bytes())
            .selector(control::EXPOSURE_MODE, &[AE_AUTO])
    }

    #[test]
    fn reads_the_mode_and_its_values() {
        let fake = exposure_camera();
        assert_eq!(fake.camera().exposure().unwrap(), auto(0.0));
        assert_eq!(fake.camera().exposure_in_effect().unwrap(), (300, 50));

        fake.poke(control::EXPOSURE_MODE, &[AE_MANUAL]);
        assert_eq!(fake.camera().exposure().unwrap(), manual(300, 50));
        assert!(matches!(
            FakeCamera::new().camera().exposure(),
            Err(Error::Unsupported)
        ));
    }

    #[test]
    fn compensation_is_signed_hundredths_of_a_stop() {
        let fake = exposure_camera();
        let camera = fake.camera();
        camera.set_exposure(&auto(-1.5)).unwrap();
        // -150 is 0xff6a, little-endian.
        assert_eq!(
            fake.xu_writes(),
            [(9, control::EXPOSURE_COMPENSATION.id, vec![0x6a, 0xff])]
        );
        assert_eq!(camera.exposure().unwrap(), auto(-1.5));
        // Unchanged values are not written again.
        camera.set_exposure(&auto(-1.5)).unwrap();
        assert_eq!(fake.xu_writes().len(), 1);
    }

    #[test]
    fn manual_values_are_written_and_rounding_is_tolerated() {
        let fake = exposure_camera();
        fake.poke(control::EXPOSURE_MODE, &[AE_MANUAL]);
        let camera = fake.camera();
        camera.set_exposure(&manual(800, 100)).unwrap();
        assert_eq!(
            fake.xu_writes(),
            [
                (9, control::ISO.id, 800u16.to_le_bytes().to_vec()),
                (9, control::SHUTTER.id, 100u16.to_le_bytes().to_vec()),
            ]
        );
        // The camera reports 1/60 s as 59, so asking for 60 again is a no-op.
        fake.poke(control::SHUTTER, &59u16.to_le_bytes());
        camera.set_exposure(&manual(800, 60)).unwrap();
        assert_eq!(fake.xu_writes().len(), 2);
    }

    #[test]
    fn setting_a_mode_switches_to_it() {
        let fake = exposure_camera();
        // The values in effect are already stored, so only the mode changes.
        fake.camera().set_exposure(&manual(300, 50)).unwrap();
        assert_eq!(
            fake.xu_writes(),
            [(9, control::EXPOSURE_MODE.id, vec![AE_MANUAL])]
        );

        let fake = exposure_camera();
        fake.camera().set_exposure(&manual(400, 50)).unwrap();
        assert_eq!(
            fake.xu_writes(),
            [
                (9, control::EXPOSURE_MODE.id, vec![AE_MANUAL]),
                (9, control::ISO.id, 400u16.to_le_bytes().to_vec()),
            ]
        );

        let fake = exposure_camera();
        fake.poke(control::EXPOSURE_MODE, &[AE_MANUAL]);
        fake.camera().set_exposure(&auto(1.0)).unwrap();
        assert_eq!(
            fake.xu_writes(),
            [
                (9, control::EXPOSURE_MODE.id, vec![AE_AUTO]),
                (9, control::EXPOSURE_COMPENSATION.id, vec![100, 0]),
            ]
        );
    }

    #[test]
    fn rejects_out_of_range_values_before_writing() {
        let fake = exposure_camera();
        for stops in [-5.5, 5.5, f64::NAN] {
            let err = fake.camera().set_exposure(&auto(stops)).unwrap_err();
            assert!(matches!(err, Error::OutOfRange { .. }));
        }
        assert!(matches!(
            fake.camera().set_exposure(&manual(400, 0)),
            Err(Error::OutOfRange {
                what: "shutter",
                ..
            })
        ));
        assert!(fake.xu_writes().is_empty());
    }

    #[test]
    fn a_dropped_write_is_reported_not_repeated() {
        // The camera sometimes ignores a write without reporting an error.
        let fake = exposure_camera();
        fake.drop_next_xu_writes(1);
        assert!(matches!(
            fake.camera().set_exposure(&auto(1.0)),
            Err(Error::NotApplied)
        ));
        assert_eq!(fake.xu_writes().len(), 1, "sent exactly once");
        // The caller decides to try again, and this time it takes.
        fake.camera().set_exposure(&auto(1.0)).unwrap();
        assert_eq!(fake.camera().exposure().unwrap(), auto(1.0));
    }

    #[test]
    fn reports_a_change_the_camera_ignores() {
        let fake = exposure_camera().swallow_xu_writes();
        assert!(matches!(
            fake.camera().set_exposure(&manual(300, 50)),
            Err(Error::NotApplied)
        ));
    }

    #[test]
    fn setting_what_the_camera_already_has_changes_nothing() {
        let fake = exposure_camera();
        fake.camera().set_exposure(&auto(0.0)).unwrap();
        assert!(fake.xu_writes().is_empty());
    }
}
