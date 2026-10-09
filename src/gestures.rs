//! Hand gestures the camera responds to.
//!
//! Each gesture is represented by a bit in a single byte. Toggling a gesture
//! requires reading the byte, updating the corresponding bit, and writing it
//! back. Bits not modeled by this crate are preserved unchanged.
//!
//! [`Features::gestures`](crate::Features::gestures) controls the master switch
//! for all gestures.

use crate::camera::Camera;
use crate::device::selectors::control;
use crate::error::{Error, Result};

const BIT_PALM: u8 = 1 << 1;
const BIT_L: u8 = 1 << 2;
const BIT_V: u8 = 1 << 3;

/// A set of gesture switches, named by what each gesture does.
///
/// As with [`ImageSettings`](crate::ImageSettings), this type is used for both
/// reading and writing. [`Camera::gestures`] populates all switches, while
/// [`Camera::set_gestures`] updates only those set to `Some`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, deny_unknown_fields)
)]
pub struct Gestures {
    /// An open palm starts and stops tracking.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub tracking: Option<bool>,
    /// An "L" shape, moved up or down, zooms in or out.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub zoom: Option<bool>,
    /// A "V" sign starts and stops whiteboard mode.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub whiteboard: Option<bool>,
}

/// The gesture switches: getter, setter and bit.
type GestureBit = (fn(&Gestures) -> Option<bool>, fn(&mut Gestures, bool), u8);

const BITS: [GestureBit; 3] = [
    (|g| g.tracking, |g, v| g.tracking = Some(v), BIT_PALM),
    (|g| g.zoom, |g, v| g.zoom = Some(v), BIT_L),
    (|g| g.whiteboard, |g, v| g.whiteboard = Some(v), BIT_V),
];

impl Camera {
    fn gesture_bits(&self) -> Result<u8> {
        let payload = self.xu_read(control::GESTURES)?;
        match payload.as_slice() {
            [bits] => Ok(*bits),
            other => Err(Error::Protocol(format!(
                "the gesture switches are {} bytes, not 1",
                other.len()
            ))),
        }
    }

    /// Reads which gestures are enabled.
    pub fn gestures(&self) -> Result<Gestures> {
        let bits = self.gesture_bits()?;
        let mut gestures = Gestures::default();
        for (_, set, bit) in BITS {
            set(&mut gestures, bits & bit != 0);
        }
        Ok(gestures)
    }

    /// Enables or disables the gestures that are `Some`, and waits until the
    /// camera reports the new state.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::NotStreaming`] unless video is streaming.
    pub fn set_gestures(&self, gestures: &Gestures) -> Result<()> {
        let (mut mask, mut value) = (0u8, 0u8);
        for (get, _, bit) in BITS {
            if let Some(on) = get(gestures) {
                mask |= bit;
                if on {
                    value |= bit;
                }
            }
        }
        if mask == 0 {
            return Ok(());
        }
        let current = self.gesture_bits()?;
        let wanted = (current & !mask) | value;
        if wanted == current {
            return Ok(());
        }
        self.require_streaming()?;
        self.write_verified(control::GESTURES, &[wanted], || {
            Ok(self.gesture_bits()? & mask == value)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeCamera;

    const DEFAULT_GESTURE_BYTE: u8 = 0x0e;

    fn gestures_camera() -> FakeCamera {
        FakeCamera::new().selector(control::GESTURES, &[DEFAULT_GESTURE_BYTE])
    }

    #[test]
    fn reads_gestures() {
        let all_on = Gestures {
            tracking: Some(true),
            zoom: Some(true),
            whiteboard: Some(true),
        };
        assert_eq!(gestures_camera().camera().gestures().unwrap(), all_on);

        let fake = gestures_camera();
        fake.poke(control::GESTURES, &[BIT_L]);
        assert_eq!(
            fake.camera().gestures().unwrap(),
            Gestures {
                tracking: Some(false),
                zoom: Some(true),
                whiteboard: Some(false),
            }
        );
    }

    #[test]
    fn changes_only_the_requested_bits() {
        let fake = gestures_camera();
        // Bits 0 and 4 belong to gestures this crate does not model.
        fake.poke(control::GESTURES, &[0x1f]);
        let camera = fake.camera();
        camera
            .set_gestures(&Gestures {
                zoom: Some(false),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(fake.xu_writes(), [(9, control::GESTURES.id, vec![0x1b])]);
        assert_eq!(camera.gestures().unwrap().zoom, Some(false));

        camera
            .set_gestures(&Gestures {
                zoom: Some(false),
                tracking: Some(true),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(fake.xu_writes().len(), 1, "nothing left to change");
        camera.set_gestures(&Gestures::default()).unwrap();
        assert_eq!(fake.xu_writes().len(), 1);
    }

    #[test]
    fn reports_a_change_the_camera_ignores() {
        let fake = gestures_camera().swallow_xu_writes();
        let no_palm = Gestures {
            tracking: Some(false),
            ..Default::default()
        };
        assert!(matches!(
            fake.camera().set_gestures(&no_palm),
            Err(Error::NotApplied)
        ));
    }

    #[test]
    fn unsupported_or_malformed() {
        let bare = FakeCamera::new().camera();
        assert!(matches!(bare.gestures(), Err(Error::Unsupported)));
        bare.set_gestures(&Gestures::default()).unwrap();
        let odd = FakeCamera::new().selector(control::GESTURES, &[0; 2]);
        assert!(matches!(odd.camera().gestures(), Err(Error::Protocol(_))));
    }
}
