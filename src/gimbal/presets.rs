//! Presets stored in the camera's internal memory.
//!
//! The camera has ten slots, each storing a name and a view consisting of the
//! mode, gimbal position, and zoom. The camera stores these presets, but the
//! host is responsible for recalling them by reading and applying their
//! settings. This module handles that process.
//!
//! Some models may support recalling presets directly, but this crate does not
//! rely on that capability.

use crate::camera::Camera;
use crate::device::selectors::ai;
use crate::error::{Error, Optional, Result};
use crate::gimbal::position::PanTiltUpdate;
use crate::mode::CameraMode;

const QUERY_OFFSET: u8 = 100;
const RECORD_LEN: usize = 127;

/// How many preset slots the camera has.
pub const PRESET_SLOTS: u8 = 10;
/// The longest preset name, in bytes of UTF-8.
pub const PRESET_NAME_MAX: usize = 63;

// Offsets within the record.
const SLOT: usize = 0;
const IN_USE: usize = 1;
const NAME: std::ops::Range<usize> = 2..66;
const MODE: usize = 66;
const PAN: std::ops::Range<usize> = 107..111;
const TILT: std::ops::Range<usize> = 111..115;
const ZOOM: std::ops::Range<usize> = 115..117;

/// Pan and tilt are stored in tenths of a degree, zoom in hundredths.
const ANGLE_SCALE: f64 = 10.0;
const ZOOM_SCALE: f64 = 100.0;

/// A stored view.
#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    /// A label of up to [`PRESET_NAME_MAX`] bytes.
    pub name: String,
    /// The camera mode.
    pub mode: CameraMode,
    /// Horizontal gimbal angle in degrees.
    pub pan: f64,
    /// Vertical gimbal angle in degrees.
    pub tilt: f64,
    /// Zoom factor.
    pub zoom: f64,
}

fn check_slot(slot: u8) -> Result<()> {
    if slot < PRESET_SLOTS {
        Ok(())
    } else {
        Err(Error::OutOfRange {
            what: "preset slot",
            value: slot.into(),
            min: 0.0,
            max: f64::from(PRESET_SLOTS - 1),
        })
    }
}

impl Preset {
    fn encode(&self, slot: u8) -> Result<Vec<u8>> {
        let name = self.name.as_bytes();
        if name.len() > PRESET_NAME_MAX || name.contains(&0) {
            return Err(Error::OutOfRange {
                what: "preset name length in bytes",
                value: name.len() as f64,
                min: 0.0,
                max: PRESET_NAME_MAX as f64,
            });
        }
        let tenths = |what, degrees: f64| {
            let raw = (degrees * ANGLE_SCALE).round();
            if raw.abs() <= 3600.0 {
                Ok(raw as i32)
            } else {
                Err(Error::OutOfRange {
                    what,
                    value: degrees,
                    min: -360.0,
                    max: 360.0,
                })
            }
        };
        let zoom = (self.zoom * ZOOM_SCALE).round();
        if !(1.0..=f64::from(u16::MAX)).contains(&zoom) {
            return Err(Error::OutOfRange {
                what: "zoom",
                value: self.zoom,
                min: 0.01,
                max: f64::from(u16::MAX) / ZOOM_SCALE,
            });
        }

        let mut record = vec![0u8; RECORD_LEN];
        record[SLOT] = slot;
        record[IN_USE] = 1;
        record[NAME.start..NAME.start + name.len()].copy_from_slice(name);
        record[MODE] = self.mode.id();
        record[PAN].copy_from_slice(&tenths("pan", self.pan)?.to_le_bytes());
        record[TILT].copy_from_slice(&tenths("tilt", self.tilt)?.to_le_bytes());
        record[ZOOM].copy_from_slice(&(zoom as u16).to_le_bytes());
        Ok(record)
    }

    /// Decodes a record, or returns `None` if its slot is not in use.
    fn decode(record: &[u8]) -> Result<Option<Self>> {
        if record.len() != RECORD_LEN {
            return Err(Error::Protocol(format!(
                "a preset record is {} bytes, not {RECORD_LEN}",
                record.len()
            )));
        }
        if record[IN_USE] == 0 {
            return Ok(None);
        }
        let name = &record[NAME];
        let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
        let mode = CameraMode::from_id(record[MODE]).ok_or_else(|| {
            Error::Protocol(format!(
                "unrecognized mode {:#04x} in a preset",
                record[MODE]
            ))
        })?;
        let word = |range: std::ops::Range<usize>| {
            i32::from_le_bytes(record[range].try_into().expect("4 bytes"))
        };
        let zoom = u16::from_le_bytes(record[ZOOM].try_into().expect("2 bytes"));
        Ok(Some(Self {
            name: String::from_utf8_lossy(name).into_owned(),
            mode,
            pan: f64::from(word(PAN)) / ANGLE_SCALE,
            tilt: f64::from(word(TILT)) / ANGLE_SCALE,
            zoom: f64::from(zoom) / ZOOM_SCALE,
        }))
    }
}

impl Camera {
    fn write_preset_record(&self, record: &[u8]) -> Result<()> {
        self.require_streaming()?;
        self.xu_write(ai::PRESET_RECORD, record)
    }

    /// Selects a slot and reads its record.
    fn read_preset_record(&self, slot: u8) -> Result<Vec<u8>> {
        let mut query = vec![0u8; RECORD_LEN];
        query[SLOT] = slot + QUERY_OFFSET;
        self.write_preset_record(&query)?;
        // The camera answers the query at once. Unused slots return all zeros,
        // so the slot number can only be verified for records in use.
        let record = self.xu_read(ai::PRESET_RECORD)?;
        match record.as_slice() {
            [other, in_use, ..] if *in_use != 0 && *other != slot => Err(Error::Protocol(format!(
                "asked for preset slot {slot}, the camera answered with slot {other}"
            ))),
            _ => Ok(record),
        }
    }

    /// Reads the preset in a slot, or `None` if the slot is empty.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::NotStreaming`] unless video is streaming.
    pub fn preset(&self, slot: u8) -> Result<Option<Preset>> {
        check_slot(slot)?;
        Preset::decode(&self.read_preset_record(slot)?)
    }

    /// Stores a preset in a slot, replacing whatever was there, and checks that
    /// the camera kept it.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::NotStreaming`] unless video is streaming.
    pub fn store_preset(&self, slot: u8, preset: &Preset) -> Result<()> {
        check_slot(slot)?;
        let record = preset.encode(slot)?;
        self.write_preset_record(&record)?;
        if self.read_preset_record(slot)? == record {
            Ok(())
        } else {
            Err(Error::NotApplied)
        }
    }

    /// Stores the current mode, gimbal position and zoom as a preset, and
    /// returns what was stored.
    ///
    /// On a camera without a gimbal the position is stored as zero.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::NotStreaming`] unless video is streaming.
    ///
    /// Fails with [`Error::Busy`] while the camera is between modes.
    pub fn save_preset(&self, slot: u8, name: &str) -> Result<Preset> {
        check_slot(slot)?;
        let position = self.position().optional()?;
        let preset = Preset {
            name: name.to_owned(),
            mode: self.mode()?,
            pan: position.map_or(0.0, |p| (p.pan * ANGLE_SCALE).round() / ANGLE_SCALE),
            tilt: position.map_or(0.0, |p| (p.tilt * ANGLE_SCALE).round() / ANGLE_SCALE),
            zoom: self.zoom()?,
        };
        self.store_preset(slot, &preset)?;
        Ok(preset)
    }

    /// Applies the preset in a slot and returns it.
    ///
    /// The mode is switched first. The position is skipped for modes that aim
    /// the gimbal themselves.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::EmptyPreset`] if the slot is empty.
    ///
    /// Fails with [`Error::OutOfRange`] if the preset holds a zoom or position
    /// this camera does not accept. Nothing is changed in that case.
    ///
    /// Fails with [`Error::NotStreaming`] unless video is streaming.
    pub fn recall_preset(&self, slot: u8) -> Result<Preset> {
        let preset = self.preset(slot)?.ok_or(Error::EmptyPreset { slot })?;
        let aims_itself = matches!(
            preset.mode,
            CameraMode::Tracking | CameraMode::Overhead | CameraMode::AutoFraming
        );
        // Checked before the mode is switched, so that a record this camera
        // cannot apply leaves it as it was.
        self.zoom_raw(preset.zoom)?;
        if !aims_itself {
            self.position_raw(preset.pan, preset.tilt).optional()?;
        }
        self.set_mode(preset.mode)?;
        self.set_zoom(preset.zoom)?;
        if !aims_itself {
            let moved = self.set_position(PanTiltUpdate {
                pan: Some(preset.pan),
                tilt: Some(preset.tilt),
            });
            moved.optional()?;
        }
        Ok(preset)
    }

    /// Empties a slot.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::NotStreaming`] unless video is streaming.
    pub fn delete_preset(&self, slot: u8) -> Result<()> {
        check_slot(slot)?;
        let mut record = vec![0u8; RECORD_LEN];
        record[SLOT] = slot;
        self.write_preset_record(&record)?;
        if self.read_preset_record(slot)?[IN_USE] == 0 {
            Ok(())
        } else {
            Err(Error::NotApplied)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::selectors::control;
    use crate::device::transport::Control;
    use crate::testing::FakeCamera;

    fn desk() -> Preset {
        Preset {
            name: "desk".into(),
            mode: CameraMode::Normal,
            pan: 20.0,
            tilt: -5.0,
            zoom: 1.5,
        }
    }

    #[test]
    fn record_layout() {
        let record = desk().encode(2).unwrap();
        assert_eq!(record.len(), RECORD_LEN);
        // Slot, in-use flag, then the name padded with zeros.
        assert_eq!(record[..6], [2, 1, b'd', b'e', b's', b'k']);
        assert!(record[6..66].iter().all(|&byte| byte == 0));
        assert_eq!(record[MODE], 0);
        // 20.0 degrees and -5.0 degrees in tenths, zoom 1.5 in hundredths.
        assert_eq!(record[107..111], [0xc8, 0, 0, 0]);
        assert_eq!(record[111..115], [0xce, 0xff, 0xff, 0xff]);
        assert_eq!(record[115..117], [0x96, 0x00]);
        // Fields this crate does not fill stay zero.
        assert!(record[67..107].iter().all(|&byte| byte == 0));
        assert!(record[117..].iter().all(|&byte| byte == 0));
        assert_eq!(Preset::decode(&record).unwrap(), Some(desk()));
    }

    #[test]
    fn empty_and_malformed_records() {
        assert_eq!(Preset::decode(&[0u8; RECORD_LEN]).unwrap(), None);
        assert!(matches!(
            Preset::decode(&[0u8; 12]),
            Err(Error::Protocol(_))
        ));
        let mut odd_mode = desk().encode(0).unwrap();
        odd_mode[MODE] = 0x33;
        assert!(matches!(Preset::decode(&odd_mode), Err(Error::Protocol(_))));
    }

    #[test]
    fn rejects_values_that_do_not_fit() {
        let long = Preset {
            name: "x".repeat(PRESET_NAME_MAX + 1),
            ..desk()
        };
        assert!(matches!(long.encode(0), Err(Error::OutOfRange { .. })));
        let exact = Preset {
            name: "x".repeat(PRESET_NAME_MAX),
            ..desk()
        };
        assert!(exact.encode(0).is_ok());
        let no_zoom = Preset {
            zoom: 0.0,
            ..desk()
        };
        assert!(matches!(
            no_zoom.encode(0),
            Err(Error::OutOfRange { what: "zoom", .. })
        ));
        assert!(matches!(check_slot(10), Err(Error::OutOfRange { .. })));
        assert!(check_slot(9).is_ok());
    }

    /// A camera that answers the next preset query with `preset`.
    fn camera_holding(preset: &Preset) -> FakeCamera {
        let fake = FakeCamera::new()
            .selector(control::VIEW_STATE, &[0u8; 61])
            .selector(ai::PRESET_RECORD, &[0u8; RECORD_LEN])
            .control(Control::Zoom, 100, 400, 1, 100)
            .control(Control::Pan, -522_000, 522_000, 3600, 0)
            .control(Control::Tilt, -324_000, 360_000, 3600, 0);
        fake.script(ai::PRESET_RECORD, [preset.encode(3).unwrap()]);
        fake
    }

    #[test]
    fn recall_applies_mode_zoom_and_position() {
        let preset = Preset {
            mode: CameraMode::DeskView,
            ..desk()
        };
        let fake = camera_holding(&preset);
        let camera = fake.camera();
        assert_eq!(camera.recall_preset(3).unwrap(), preset);
        assert_eq!(camera.mode().unwrap(), CameraMode::DeskView);
        assert_eq!(fake.get(Control::Zoom), 150);
        assert_eq!(
            (fake.get(Control::Pan), fake.get(Control::Tilt)),
            (72_000, -18_000)
        );
    }

    #[test]
    fn recall_changes_nothing_when_the_record_does_not_fit_the_camera() {
        // Records another program could have stored: a zoom beyond this
        // camera's lens and a pan beyond its gimbal.
        for (what, preset) in [
            (
                "zoom",
                Preset {
                    mode: CameraMode::DeskView,
                    zoom: 6.0,
                    ..desk()
                },
            ),
            (
                "pan",
                Preset {
                    mode: CameraMode::DeskView,
                    pan: 170.0,
                    ..desk()
                },
            ),
        ] {
            let fake = camera_holding(&preset);
            let camera = fake.camera();
            let err = camera.recall_preset(3).unwrap_err();
            assert!(
                matches!(err, Error::OutOfRange { what: found, .. } if found == what),
                "{err:?}"
            );
            // Only the query for the slot was sent.
            assert_eq!(fake.xu_writes().len(), 1);
            assert!(fake.writes().is_empty());
            assert_eq!(camera.mode().unwrap(), CameraMode::Normal);
        }
    }

    #[test]
    fn recall_ignores_the_position_in_modes_that_aim_themselves() {
        let preset = Preset {
            mode: CameraMode::Overhead,
            pan: 170.0,
            ..desk()
        };
        let fake = camera_holding(&preset);
        let camera = fake.camera();
        camera.recall_preset(3).unwrap();
        assert_eq!(camera.mode().unwrap(), CameraMode::Overhead);
        assert_eq!(fake.writes(), [[(Control::Zoom, 150)]]);
    }
}
