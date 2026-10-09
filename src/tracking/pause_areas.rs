//! Pause-track zones where the camera stops tracking.
//!
//! The camera supports up to six zones and a switch to enable or disable them.
//! Each zone consists of a rectangle in the image and the gimbal angle at which
//! it was defined.
//!
//! When the tracked person enters a zone, the camera turns to face the middle
//! of the zone and holds that position until the person leaves, then resumes
//! tracking.

use crate::camera::Camera;
use crate::device::selectors::aux;
use crate::error::{Error, Result};
use crate::features::BIT_PAUSE_TRACK;

const RECORD_LEN: usize = 15;

/// How many zones the camera holds.
pub const PAUSE_AREA_SLOTS: usize = 6;

/// Angles are stored in tenths of a degree, rectangle edges in ten-thousandths
/// of the frame.
const ANGLE_SCALE: f64 = 10.0;
const FRAME_SCALE: f64 = 10_000.0;
const ACTIVE: u8 = 0x80;

/// A zone in which tracking pauses.
///
/// The rectangle uses normalized frame coordinates with the origin at the
/// top-left corner, relative to the view at the specified pan and tilt angles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PauseArea {
    /// Whether the zone is active.
    pub active: bool,
    /// Horizontal gimbal angle in degrees.
    pub pan: f64,
    /// Vertical gimbal angle in degrees.
    pub tilt: f64,
    /// Left edge as a fraction of the frame width.
    pub x: f64,
    /// Top edge as a fraction of the frame height.
    pub y: f64,
    /// Width as a fraction of the frame width.
    pub width: f64,
    /// Height as a fraction of the frame height.
    pub height: f64,
}

impl PauseArea {
    fn encode(&self, id: u8) -> Result<[u8; RECORD_LEN]> {
        let angle = |what, degrees: f64| {
            let raw = (degrees * ANGLE_SCALE).round();
            if raw.abs() <= 1800.0 {
                Ok(raw as i16)
            } else {
                Err(Error::OutOfRange {
                    what,
                    value: degrees,
                    min: -180.0,
                    max: 180.0,
                })
            }
        };
        let fraction = |what, value: f64| {
            if (0.0..=1.0).contains(&value) {
                Ok((value * FRAME_SCALE).round() as u16)
            } else {
                Err(Error::OutOfRange {
                    what,
                    value,
                    min: 0.0,
                    max: 1.0,
                })
            }
        };
        for (what, start, size) in [
            ("pause area right edge", self.x, self.width),
            ("pause area bottom edge", self.y, self.height),
        ] {
            // Also catches NaN, which fails every comparison.
            if (start + size).partial_cmp(&1.0) == Some(std::cmp::Ordering::Greater) || size <= 0.0
            {
                return Err(Error::OutOfRange {
                    what,
                    value: start + size,
                    min: 0.0,
                    max: 1.0,
                });
            }
        }
        let mut record = [0u8; RECORD_LEN];
        record[0] = id | if self.active { ACTIVE } else { 0 };
        record[3..5].copy_from_slice(&angle("pan", self.pan)?.to_le_bytes());
        record[5..7].copy_from_slice(&angle("tilt", self.tilt)?.to_le_bytes());
        record[7..9].copy_from_slice(&fraction("pause area x", self.x)?.to_le_bytes());
        record[9..11].copy_from_slice(&fraction("pause area y", self.y)?.to_le_bytes());
        record[11..13].copy_from_slice(&fraction("pause area width", self.width)?.to_le_bytes());
        record[13..15].copy_from_slice(&fraction("pause area height", self.height)?.to_le_bytes());
        Ok(record)
    }

    /// Decodes a record, or returns `None` for an unused one, which has no
    /// size.
    fn decode(record: &[u8]) -> Option<Self> {
        let word = |at: usize| u16::from_le_bytes([record[at], record[at + 1]]);
        let (width, height) = (word(11), word(13));
        if width == 0 || height == 0 {
            return None;
        }
        Some(Self {
            active: record[0] & ACTIVE != 0,
            pan: f64::from(word(3) as i16) / ANGLE_SCALE,
            tilt: f64::from(word(5) as i16) / ANGLE_SCALE,
            x: f64::from(word(7)) / FRAME_SCALE,
            y: f64::from(word(9)) / FRAME_SCALE,
            width: f64::from(width) / FRAME_SCALE,
            height: f64::from(height) / FRAME_SCALE,
        })
    }
}

impl Camera {
    /// Whether the pause-track areas are in force.
    pub fn pause_areas_enabled(&self) -> Result<bool> {
        Ok(self.feature_bits()? & BIT_PAUSE_TRACK != 0)
    }

    /// Sets whether pause-track areas are enabled, and waits until the camera
    /// reports the new state.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::NotStreaming`] unless video is streaming.
    pub fn set_pause_areas_enabled(&self, enabled: bool) -> Result<()> {
        let value = if enabled { BIT_PAUSE_TRACK } else { 0 };
        self.set_feature_bits(BIT_PAUSE_TRACK, value)
    }

    /// Reads the zones the camera holds, in slot order.
    pub fn pause_areas(&self) -> Result<Vec<PauseArea>> {
        let payload = self.xu_read(aux::PAUSE_AREAS)?;
        if payload.len() != RECORD_LEN * PAUSE_AREA_SLOTS {
            return Err(Error::Protocol(format!(
                "the pause-track areas are {} bytes, not {}",
                payload.len(),
                RECORD_LEN * PAUSE_AREA_SLOTS
            )));
        }
        let (records, _) = payload.as_chunks::<RECORD_LEN>();
        Ok(records
            .iter()
            .filter_map(|record| PauseArea::decode(record))
            .collect())
    }

    /// Replaces the camera's zones with `areas`, at most [`PAUSE_AREA_SLOTS`]
    /// of them, and checks that the camera kept them. An empty slice clears
    /// them.
    ///
    /// This does not change whether the zones are in force. See
    /// [`Camera::set_pause_areas_enabled`] for that.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::NotStreaming`] unless video is streaming.
    pub fn set_pause_areas(&self, areas: &[PauseArea]) -> Result<()> {
        if areas.len() > PAUSE_AREA_SLOTS {
            return Err(Error::OutOfRange {
                what: "number of pause-track areas",
                value: areas.len() as f64,
                min: 0.0,
                max: PAUSE_AREA_SLOTS as f64,
            });
        }
        let mut payload = vec![0u8; RECORD_LEN * PAUSE_AREA_SLOTS];
        for (index, area) in areas.iter().enumerate() {
            // Zones are numbered from 1. An unused record is all zeros.
            let record = area.encode(index as u8 + 1)?;
            payload[index * RECORD_LEN..(index + 1) * RECORD_LEN].copy_from_slice(&record);
        }
        self.require_streaming()?;
        self.write_verified(aux::PAUSE_AREAS, &payload, || {
            Ok(self.xu_read(aux::PAUSE_AREAS)? == payload)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::selectors::control;
    use crate::testing::FakeCamera;

    fn left_half() -> PauseArea {
        PauseArea {
            active: true,
            pan: 70.0,
            tilt: 0.0,
            x: 0.0,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        }
    }

    #[test]
    fn record_matches_a_captured_one() {
        // Written to a camera by hand and read back unchanged.
        let expected = [
            0x81, 0x00, 0x00, 0xbc, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x88, 0x13, 0x10,
            0x27,
        ];
        assert_eq!(left_half().encode(1).unwrap(), expected);
        assert_eq!(PauseArea::decode(&expected), Some(left_half()));
        assert_eq!(PauseArea::decode(&[0u8; RECORD_LEN]), None);
    }

    #[test]
    fn negative_angles_and_inner_rectangles_round_trip() {
        let area = PauseArea {
            active: false,
            pan: -30.0,
            tilt: 10.0,
            x: 0.25,
            y: 0.25,
            width: 0.5,
            height: 0.5,
        };
        let record = area.encode(2).unwrap();
        assert_eq!(record[..7], [0x02, 0x00, 0x00, 0xd4, 0xfe, 0x64, 0x00]);
        assert_eq!(PauseArea::decode(&record), Some(area));
    }

    #[test]
    fn rejects_rectangles_outside_the_frame() {
        for area in [
            PauseArea {
                x: 0.6,
                ..left_half()
            },
            PauseArea {
                width: 0.0,
                ..left_half()
            },
            PauseArea {
                y: -0.1,
                ..left_half()
            },
            PauseArea {
                height: f64::NAN,
                ..left_half()
            },
            PauseArea {
                pan: 200.0,
                ..left_half()
            },
        ] {
            assert!(
                matches!(area.encode(1), Err(Error::OutOfRange { .. })),
                "{area:?}"
            );
        }
    }

    #[test]
    fn stores_reads_and_clears_zones() {
        let fake = FakeCamera::new()
            .selector(aux::PAUSE_AREAS, &[0; 90])
            .selector(control::FEATURES, &[0x0a, 0xd5]);
        let camera = fake.camera();
        assert!(camera.pause_areas().unwrap().is_empty());

        let second = PauseArea {
            pan: -30.0,
            x: 0.5,
            ..left_half()
        };
        camera.set_pause_areas(&[left_half(), second]).unwrap();
        assert_eq!(camera.pause_areas().unwrap(), [left_half(), second]);
        // Records are numbered from 1 and the rest stay zero.
        let stored = &fake.xu_writes()[0].2;
        assert_eq!((stored[0], stored[15]), (0x81, 0x82));
        assert!(stored[30..].iter().all(|&byte| byte == 0));

        camera.set_pause_areas(&[]).unwrap();
        assert!(camera.pause_areas().unwrap().is_empty());
        assert!(matches!(
            camera.set_pause_areas(&[left_half(); 7]),
            Err(Error::OutOfRange { .. })
        ));
    }

    #[test]
    fn the_switch_is_one_feature_bit() {
        let fake = FakeCamera::new().selector(control::FEATURES, &[0xd5, 0x0a]);
        let camera = fake.camera();
        assert!(!camera.pause_areas_enabled().unwrap());
        camera.set_pause_areas_enabled(true).unwrap();
        // 0x0ad5 with bit 14 set, every other bit untouched.
        assert_eq!(
            fake.xu_writes(),
            [(9, control::FEATURES.id, vec![0xd5, 0x4a])]
        );
        assert!(camera.pause_areas_enabled().unwrap());
    }
}
