//! The tracking area: bounds the gimbal stays within while tracking.
//!
//! Only supported on the Link 2 Pro. Other cameras may have the selector but
//! ignore what is written to it. On those, [`Camera::set_tracking_area`] fails
//! with [`Error::NotApplied`] and [`Camera::tracking_area`] with
//! [`Error::Unsupported`].

use std::ops::RangeInclusive;

use crate::camera::Camera;
use crate::device::selectors::ai;
use crate::error::{Error, Result};

const AREA_LEN: usize = 19;
const ANGLE_SCALE: f64 = 10.0;

/// The default for the four angles used when the camera is turned to portrait:
/// tilt -45 to 45 and pan -30 to 30 degrees.
const PORTRAIT_DEFAULTS: [i16; 4] = [-450, 450, 300, -300];

/// Where tracking may take the gimbal, in degrees.
///
/// Only supported on the Link 2 Pro.
///
/// The default is 100 degrees either way horizontally and 30 degrees up and
/// down.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackingArea {
    /// Whether the limits are in force.
    pub enabled: bool,
    /// Horizontal limits, leftmost to rightmost.
    pub pan: RangeInclusive<f64>,
    /// Vertical limits, lowest to highest.
    pub tilt: RangeInclusive<f64>,
}

impl Default for TrackingArea {
    fn default() -> Self {
        Self {
            enabled: false,
            pan: -100.0..=100.0,
            tilt: -30.0..=30.0,
        }
    }
}

impl TrackingArea {
    fn encode(&self) -> Result<[u8; AREA_LEN]> {
        let tenths = |what, degrees: f64| {
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
        let limits = |what, range: &RangeInclusive<f64>| {
            let (start, end) = (tenths(what, *range.start())?, tenths(what, *range.end())?);
            // Compared as encoded, since limits that round to the same tenth
            // describe no area.
            if start < end {
                Ok((start, end))
            } else {
                Err(Error::InvalidRange {
                    what,
                    min: *range.start(),
                    max: *range.end(),
                })
            }
        };
        let (pan_start, pan_end) = limits("pan limits", &self.pan)?;
        let (tilt_start, tilt_end) = limits("tilt limits", &self.tilt)?;
        let angles = [pan_start, pan_end, tilt_end, tilt_start];
        let mut payload = [0u8; AREA_LEN];
        payload[0] = u8::from(self.enabled);
        for (index, angle) in angles.into_iter().chain(PORTRAIT_DEFAULTS).enumerate() {
            payload[2 + 2 * index..4 + 2 * index].copy_from_slice(&angle.to_le_bytes());
        }
        Ok(payload)
    }

    /// Decodes a payload, or returns `None` if it does not describe an area, as
    /// on cameras that ignore the selector.
    fn decode(payload: &[u8]) -> Option<Self> {
        if payload.len() != AREA_LEN || payload[0] > 1 {
            return None;
        }
        let angle = |index: usize| {
            let raw = i16::from_le_bytes([payload[2 + 2 * index], payload[3 + 2 * index]]);
            f64::from(raw) / ANGLE_SCALE
        };
        let (pan, tilt) = (angle(0)..=angle(1), angle(3)..=angle(2));
        let sane = |range: &RangeInclusive<f64>| {
            range.start() < range.end()
                && range.start().abs() <= 180.0
                && range.end().abs() <= 180.0
        };
        (sane(&pan) && sane(&tilt)).then_some(Self {
            enabled: payload[0] == 1,
            pan,
            tilt,
        })
    }
}

impl Camera {
    /// Reads the tracking area.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::Unsupported`] if the camera lacks the selector or
    /// answers with something that is not an area.
    pub fn tracking_area(&self) -> Result<TrackingArea> {
        let payload = self.xu_read(ai::TRACKING_AREA)?;
        TrackingArea::decode(&payload).ok_or(Error::Unsupported)
    }

    /// Sets the tracking area, and checks that the camera kept it.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::NotStreaming`] unless video is streaming.
    ///
    /// Fails with [`Error::NotApplied`] if the camera did not keep the area.
    /// A camera that does not support tracking areas always answers this way,
    /// and one that does can still drop a write.
    pub fn set_tracking_area(&self, area: &TrackingArea) -> Result<()> {
        let payload = area.encode()?;
        self.require_streaming()?;
        self.write_verified(ai::TRACKING_AREA, &payload, || {
            let stored = self.xu_read(ai::TRACKING_AREA)?;
            // The last byte and the second flag are the camera's to set.
            Ok(stored.len() == AREA_LEN
                && stored[0] == payload[0]
                && stored[2..18] == payload[2..18])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeCamera;

    #[test]
    fn default_area_encodes_as_link_controller_resets_it() {
        // Enabled, then -1000, 1000, 300, -300, -450, 450, 300, -300.
        let area = TrackingArea {
            enabled: true,
            ..Default::default()
        };
        let expected = [
            0x01, 0x00, 0x18, 0xfc, 0xe8, 0x03, 0x2c, 0x01, 0xd4, 0xfe, 0x3e, 0xfe, 0xc2, 0x01,
            0x2c, 0x01, 0xd4, 0xfe, 0x00,
        ];
        assert_eq!(area.encode().unwrap(), expected);
        assert_eq!(TrackingArea::decode(&expected), Some(area));
    }

    #[test]
    fn rejects_inverted_or_oversized_limits() {
        for (pan, tilt) in [
            (10.0..=-10.0, -30.0..=30.0),
            (-10.0..=10.0, 5.0..=5.0),
            (10.0..=10.04, -30.0..=30.0),
            (-200.0..=10.0, -30.0..=30.0),
            (f64::NAN..=10.0, -30.0..=30.0),
        ] {
            let area = TrackingArea {
                enabled: true,
                pan,
                tilt,
            };
            assert!(matches!(
                area.encode(),
                Err(Error::OutOfRange { .. } | Error::InvalidRange { .. })
            ));
        }
    }

    #[test]
    fn stores_and_reads_back_on_a_camera_that_supports_it() {
        let fake = FakeCamera::new().selector(ai::TRACKING_AREA, &[0; AREA_LEN]);
        let camera = fake.camera();
        // All zeros is not an area.
        assert!(matches!(camera.tracking_area(), Err(Error::Unsupported)));
        let area = TrackingArea {
            enabled: true,
            pan: -60.0..=45.5,
            tilt: -20.0..=10.0,
        };
        camera.set_tracking_area(&area).unwrap();
        assert_eq!(camera.tracking_area().unwrap(), area);
    }

    #[test]
    fn a_camera_without_the_feature_ignores_the_write() {
        // What a camera without the feature returned whatever was written.
        let filler = [
            0x00, 0x00, 0x00, 0x05, 0x84, 0x68, 0x90, 0x01, 0x50, 0x20, 0x88, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let fake = FakeCamera::new()
            .selector(ai::TRACKING_AREA, &filler)
            .swallow_xu_writes();
        let camera = fake.camera();
        assert!(matches!(camera.tracking_area(), Err(Error::Unsupported)));
        assert!(matches!(
            camera.set_tracking_area(&TrackingArea::default()),
            Err(Error::NotApplied)
        ));
    }
}
