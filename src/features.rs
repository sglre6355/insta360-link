//! Feature switches: HDR, image flipping, AI Zoom, and more.
//!
//! Most features are represented by bits in a single 16-bit value. Toggling a
//! feature requires reading the value, updating the corresponding bit, and
//! writing it back. Bits not modeled by this crate are preserved unchanged.
//! Modifying some of them causes the camera to restart.

use crate::camera::Camera;
use crate::device::selectors::{Selector, aux, control};
use crate::error::{Error, Optional, Result};

const BIT_AI_ZOOM: u16 = 1 << 0;
const BIT_HDR: u16 = 1 << 2;
const BIT_HORIZONTAL_FLIP: u16 = 1 << 3;
const BIT_GESTURES: u16 = 1 << 4;
const BIT_SMART_ADJUSTMENT: u16 = 1 << 9;
const BIT_MUTE_IN_PRIVACY: u16 = 1 << 11;
const BIT_VERTICAL_FLIP: u16 = 1 << 12;
/// Puts the pause-track areas in force. It is not part of [`Features`] because
/// [`Camera::set_pause_areas_enabled`] switches it.
pub(crate) const BIT_PAUSE_TRACK: u16 = 1 << 14;

/// A set of feature switches.
///
/// As with [`ImageSettings`](crate::ImageSettings), this type is used for both
/// reading and writing. [`Camera::features`] populates all switches, while
/// [`Camera::set_features`] updates only those set to `Some`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, deny_unknown_fields)
)]
pub struct Features {
    /// Automatically adjusts zoom while tracking to frame the subject according
    /// to the [`Framing`](crate::Framing) style.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub ai_zoom: Option<bool>,
    /// Captures the picture in high dynamic range.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub hdr: Option<bool>,
    /// Flips the picture horizontally.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub horizontal_flip: Option<bool>,
    /// Flips the picture vertically. Takes a few seconds to apply.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub vertical_flip: Option<bool>,
    /// Responds to hand gestures. With this off the camera ignores every
    /// gesture, whatever [`Gestures`](crate::Gestures) allows.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub gestures: Option<bool>,
    /// Keeps the person centered with small corrections.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub smart_adjustment: Option<bool>,
    /// Mutes the microphone while the camera is in privacy mode.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub mute_in_privacy: Option<bool>,
    /// Cancels background noise picked up by the microphone.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub noise_canceling: Option<bool>,
    /// Levels the picture when the camera is not sitting level.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub horizontal_correction: Option<bool>,
}

/// A switch held in the feature bitmask: getter, setter and bit.
type BitSwitch = (fn(&Features) -> Option<bool>, fn(&mut Features, bool), u16);

const BITS: [BitSwitch; 7] = [
    (|f| f.ai_zoom, |f, v| f.ai_zoom = Some(v), BIT_AI_ZOOM),
    (|f| f.hdr, |f, v| f.hdr = Some(v), BIT_HDR),
    (
        |f| f.horizontal_flip,
        |f, v| f.horizontal_flip = Some(v),
        BIT_HORIZONTAL_FLIP,
    ),
    (
        |f| f.vertical_flip,
        |f, v| f.vertical_flip = Some(v),
        BIT_VERTICAL_FLIP,
    ),
    (|f| f.gestures, |f, v| f.gestures = Some(v), BIT_GESTURES),
    (
        |f| f.smart_adjustment,
        |f, v| f.smart_adjustment = Some(v),
        BIT_SMART_ADJUSTMENT,
    ),
    (
        |f| f.mute_in_privacy,
        |f, v| f.mute_in_privacy = Some(v),
        BIT_MUTE_IN_PRIVACY,
    ),
];

impl Features {
    fn from_bits(bits: u16) -> Self {
        let mut features = Self::default();
        for (_, set, bit) in BITS {
            set(&mut features, bits & bit != 0);
        }
        features
    }

    /// Returns the bitmask and values for switches set to `Some`.
    ///
    /// The first mask identifies the bits to update, while the second specifies
    /// which of those bits should be set.
    fn mask_and_value(&self) -> (u16, u16) {
        let (mut mask, mut value) = (0, 0);
        for (get, _, bit) in BITS {
            if let Some(on) = get(self) {
                mask |= bit;
                if on {
                    value |= bit;
                }
            }
        }
        (mask, value)
    }
}

impl Camera {
    pub(crate) fn feature_bits(&self) -> Result<u16> {
        let payload = self.xu_read(control::FEATURES)?;
        <[u8; 2]>::try_from(payload.as_slice())
            .map(u16::from_le_bytes)
            .map_err(|_| {
                Error::Protocol(format!(
                    "the feature switches are {} bytes, not 2",
                    payload.len()
                ))
            })
    }

    /// Reads a switch from the first byte of its selector's payload.
    fn switch_selector(&self, selector: Selector) -> Result<bool> {
        let payload = self.xu_read(selector)?;
        payload
            .first()
            .map(|&byte| byte != 0)
            .ok_or_else(|| Error::Protocol(format!("{selector} is 0 bytes")))
    }

    /// Sets a switch using the first byte of its selector's payload, zeroing
    /// the remaining bytes.
    fn set_switch_selector(&self, selector: Selector, on: bool) -> Result<()> {
        let mut payload = self.xu_read(selector)?;
        match payload.first() {
            None => return Err(Error::Protocol(format!("{selector} is 0 bytes"))),
            Some(&byte) if (byte != 0) == on => return Ok(()),
            Some(_) => {}
        }
        self.require_streaming()?;
        payload.fill(0);
        payload[0] = u8::from(on);
        self.write_verified(selector, &payload, || {
            Ok(self.switch_selector(selector)? == on)
        })
    }

    /// Reads the feature switches. Switches the camera lacks are `None`.
    pub fn features(&self) -> Result<Features> {
        let mut features = self
            .feature_bits()
            .optional()?
            .map(Features::from_bits)
            .unwrap_or_default();
        features.noise_canceling = self.switch_selector(control::NOISE_CANCELING).optional()?;
        features.horizontal_correction = self
            .switch_selector(aux::HORIZONTAL_CORRECTION)
            .optional()?;
        Ok(features)
    }

    /// Changes the feature switches that are `Some`, and waits until the camera
    /// reports the new state.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::NotStreaming`] unless video is streaming.
    pub fn set_features(&self, features: &Features) -> Result<()> {
        let (mask, value) = features.mask_and_value();
        self.set_feature_bits(mask, value)?;

        if let Some(on) = features.noise_canceling {
            self.set_switch_selector(control::NOISE_CANCELING, on)?;
        }
        if let Some(on) = features.horizontal_correction {
            self.set_switch_selector(aux::HORIZONTAL_CORRECTION, on)?;
        }
        Ok(())
    }

    /// Updates the bits specified by `mask` according to `value`, preserving
    /// all other bits, and waits until the camera reports the new state. An
    /// empty mask has no effect.
    pub(crate) fn set_feature_bits(&self, mask: u16, value: u16) -> Result<()> {
        if mask == 0 {
            return Ok(());
        }
        let current = self.feature_bits()?;
        let wanted = (current & !mask) | (value & mask);
        if wanted == current {
            return Ok(());
        }
        self.require_streaming()?;
        self.xu_write(control::FEATURES, &wanted.to_le_bytes())?;
        // Compare only the requested bits. The camera changes others on its
        // own.
        self.settle(|| Ok(self.feature_bits()? & mask == value & mask))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeCamera;

    /// Hand gestures on and every other switch off.
    const DEFAULT_FEATURE_BYTES: [u8; 2] = [0x10, 0x00];

    fn features_camera() -> FakeCamera {
        FakeCamera::new()
            .selector(control::FEATURES, &DEFAULT_FEATURE_BYTES)
            .selector(control::NOISE_CANCELING, &[1])
            .selector(aux::HORIZONTAL_CORRECTION, &[0; 25])
    }

    #[test]
    fn reads_switches() {
        assert_eq!(
            features_camera().camera().features().unwrap(),
            Features {
                ai_zoom: Some(false),
                hdr: Some(false),
                horizontal_flip: Some(false),
                vertical_flip: Some(false),
                gestures: Some(true),
                smart_adjustment: Some(false),
                mute_in_privacy: Some(false),
                noise_canceling: Some(true),
                horizontal_correction: Some(false),
            }
        );
        // 0x1a0d: AI Zoom, HDR, horizontal flip, smart adjustment, mute in
        // privacy mode, and vertical flip enabled, and hand gestures disabled.
        let fake = features_camera();
        fake.poke(control::FEATURES, &[0x0d, 0x1a]);
        fake.poke(control::NOISE_CANCELING, &[0]);
        assert_eq!(
            fake.camera().features().unwrap(),
            Features {
                ai_zoom: Some(true),
                hdr: Some(true),
                horizontal_flip: Some(true),
                vertical_flip: Some(true),
                gestures: Some(false),
                smart_adjustment: Some(true),
                mute_in_privacy: Some(true),
                noise_canceling: Some(false),
                horizontal_correction: Some(false),
            }
        );
    }

    #[test]
    fn updates_requested_bits() {
        let fake = features_camera();
        let camera = fake.camera();
        camera
            .set_features(&Features {
                hdr: Some(true),
                gestures: Some(false),
                vertical_flip: Some(true),
                ..Default::default()
            })
            .unwrap();
        // Bits 2 and 12 set and bit 4 cleared.
        assert_eq!(
            fake.xu_writes(),
            [(9, control::FEATURES.id, vec![0x04, 0x10])]
        );
        assert_eq!(camera.features().unwrap().hdr, Some(true));
    }

    #[test]
    fn preserves_unrequested_bits() {
        // Some bits not modeled by this crate can restart the camera when
        // changed. Test with all bits set and all bits cleared to catch
        // unintended changes in either direction.
        for start in [u16::MAX, 0] {
            let fake = features_camera();
            fake.poke(control::FEATURES, &start.to_le_bytes());
            let flip_hdr = Features {
                hdr: Some(start & BIT_HDR == 0),
                ..Default::default()
            };
            fake.camera().set_features(&flip_hdr).unwrap();
            let expected = (start ^ BIT_HDR).to_le_bytes().to_vec();
            assert_eq!(
                fake.xu_writes(),
                [(9, control::FEATURES.id, expected)],
                "starting from {start:#06x}"
            );
        }
    }

    #[test]
    fn noise_canceling_has_its_own_selector() {
        let fake = features_camera();
        let camera = fake.camera();
        let off = Features {
            noise_canceling: Some(false),
            ..Default::default()
        };
        camera.set_features(&off).unwrap();
        assert_eq!(
            fake.xu_writes(),
            [(9, control::NOISE_CANCELING.id, vec![0])]
        );
        assert_eq!(camera.features().unwrap().noise_canceling, Some(false));
        camera.set_features(&off).unwrap();
        assert_eq!(fake.xu_writes().len(), 1, "already off: no write");
    }

    #[test]
    fn horizontal_correction_is_the_first_byte_of_a_longer_selector() {
        let fake = features_camera();
        let camera = fake.camera();
        let on = Features {
            horizontal_correction: Some(true),
            ..Default::default()
        };
        camera.set_features(&on).unwrap();
        let mut payload = vec![0u8; 25];
        payload[0] = 1;
        assert_eq!(
            fake.xu_writes(),
            [(10, aux::HORIZONTAL_CORRECTION.id, payload)]
        );
        assert_eq!(camera.features().unwrap().horizontal_correction, Some(true));
    }

    #[test]
    fn no_write_when_nothing_changes() {
        let fake = features_camera();
        fake.camera()
            .set_features(&Features {
                gestures: Some(true),
                ..Default::default()
            })
            .unwrap();
        fake.camera().set_features(&Features::default()).unwrap();
        assert!(fake.xu_writes().is_empty());
        // An empty change is fine even on a camera without the switches.
        let bare = FakeCamera::new().camera();
        bare.set_features(&Features::default()).unwrap();
    }

    #[test]
    fn reports_a_change_the_camera_ignores() {
        let fake = features_camera().swallow_xu_writes();
        let hdr_on = Features {
            hdr: Some(true),
            ..Default::default()
        };
        assert!(matches!(
            fake.camera().set_features(&hdr_on),
            Err(Error::NotApplied)
        ));
    }

    #[test]
    fn missing_selectors_read_as_none_and_malformed_ones_fail() {
        let bare = FakeCamera::new().camera();
        assert_eq!(bare.features().unwrap(), Features::default());
        let hdr_on = Features {
            hdr: Some(true),
            ..Default::default()
        };
        assert!(matches!(
            bare.set_features(&hdr_on),
            Err(Error::Unsupported)
        ));
        let odd = FakeCamera::new().selector(control::FEATURES, &[0; 4]);
        assert!(matches!(odd.camera().features(), Err(Error::Protocol(_))));
    }
}
