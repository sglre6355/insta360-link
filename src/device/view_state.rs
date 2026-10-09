//! View state reported by selector 2 of the vendor control unit.
//!
//! The first byte identifies the camera mode, while the second reports its
//! status. Newer cameras use a longer, 61-byte payload that also reports the
//! actual pan, tilt, and zoom values and accepts target values when written.

use std::ops::Range;

use super::selectors::control;
use crate::camera::Camera;
use crate::error::{Error, Result};
use crate::model::Model;

/// Camera mode, either reported or requested.
const MODE: usize = 0;
/// Status of the reported mode. The camera ignores writes to this byte.
const STATUS: usize = 1;
/// Actual zoom in hundredths, stored as a little-endian `u16`.
/// Only present in the longer payload.
const ZOOM: Range<usize> = 50..52;

/// Camera status flags, present only in the longer payload.
/// Bit 0 indicates privacy mode.
const FLAGS: usize = 54;
const FLAG_PRIVACY: u8 = 1 << 0;

/// Offset of the target pan in a mode request. An unset value lets the camera
/// choose its position. Zero would instead move the gimbal to the center.
///
/// Cameras with a gimbal use a little-endian `i32`. Link 2C models use two
/// `i16` values instead, while the original Link leaves this field empty.
const TARGET_PAN: usize = 38;
/// Offsets of the target tilt and third angle, stored as little-endian `i32`s.
const TARGET_ANGLES: [usize; 2] = [42, 46];
/// Host pitch in tenths of a degree, stored as a little-endian `i16`. The
/// preceding `u16` zoom and following `u16` flags remain zero.
const TARGET_HOST_PITCH: usize = 52;

/// Sentinel value indicating an unspecified angle.
const UNSET_ANGLE: i32 = 3610;
/// Sentinel pair indicating an unspecified target on Link 2C models.
const UNSET_PAIR: [i16; 2] = [-1, -1];
const DEFAULT_HOST_PITCH: i16 = -450;

/// The view state as the camera reported it.
pub(crate) struct ViewState(Vec<u8>);

impl ViewState {
    fn byte(&self, at: usize) -> Result<u8> {
        self.0.get(at).copied().ok_or_else(|| {
            Error::Protocol(format!("the view state is only {} bytes", self.0.len()))
        })
    }

    /// The byte that says which mode the camera is in.
    pub(crate) fn mode_id(&self) -> Result<u8> {
        self.byte(MODE)
    }

    /// The byte that reports how the mode is getting on.
    pub(crate) fn status(&self) -> Result<u8> {
        self.byte(STATUS)
    }

    /// The zoom the camera is using, in hundredths, or `None` if the payload is
    /// too short to say.
    pub(crate) fn zoom(&self) -> Option<u16> {
        self.0
            .get(ZOOM)
            .map(|bytes| u16::from_le_bytes(bytes.try_into().expect("2 bytes")))
    }

    /// Whether the camera is in privacy mode, or `None` if the payload is too
    /// short to say.
    pub(crate) fn privacy(&self) -> Option<bool> {
        self.0.get(FLAGS).map(|flags| flags & FLAG_PRIVACY != 0)
    }

    /// Builds a mode request using this view state's payload format.
    ///
    /// The camera chooses the position and zoom. Fields that do not fit in
    /// shorter payloads are omitted.
    pub(crate) fn mode_request(&self, mode_id: u8, model: Model) -> Result<Vec<u8>> {
        self.status()?;
        let mut payload = vec![0u8; self.0.len()];
        payload[MODE] = mode_id;
        let mut put = |at: usize, bytes: &[u8]| {
            if let Some(field) = payload.get_mut(at..at + bytes.len()) {
                field.copy_from_slice(bytes);
            }
        };
        match model {
            Model::Link => {}
            Model::Link2C | Model::Link2CPro => {
                put(TARGET_PAN, &UNSET_PAIR[0].to_le_bytes());
                put(TARGET_PAN + 2, &UNSET_PAIR[1].to_le_bytes());
            }
            _ => put(TARGET_PAN, &UNSET_ANGLE.to_le_bytes()),
        }
        for at in TARGET_ANGLES {
            put(at, &UNSET_ANGLE.to_le_bytes());
        }
        put(TARGET_HOST_PITCH, &DEFAULT_HOST_PITCH.to_le_bytes());
        Ok(payload)
    }
}

impl Camera {
    /// Reads the view state.
    pub(crate) fn view_state(&self) -> Result<ViewState> {
        self.xu_read(control::VIEW_STATE).map(ViewState)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_mode_status_and_zoom_of_the_longer_payload() {
        let mut payload = vec![0u8; 61];
        payload[..2].copy_from_slice(&[0x06, 0x10]);
        payload[50..52].copy_from_slice(&180u16.to_le_bytes());
        // The host pitch follows the zoom and is not part of it.
        payload[52..54].copy_from_slice(&(-450i16).to_le_bytes());
        let state = ViewState(payload);
        assert_eq!(state.mode_id().unwrap(), 0x06);
        assert_eq!(state.status().unwrap(), 0x10);
        assert_eq!(state.zoom(), Some(180));
    }

    #[test]
    fn reads_the_privacy_flag() {
        // Captured in and out of privacy mode. The other bits of the field are
        // not understood.
        for (flags, privacy) in [(0x23, true), (0x22, false)] {
            let mut payload = vec![0u8; 61];
            payload[54] = flags;
            assert_eq!(ViewState(payload).privacy(), Some(privacy));
        }
        // A payload that ends before the flags.
        assert_eq!(ViewState(vec![0xff; 52]).privacy(), None);
    }

    #[test]
    fn payloads_too_short_for_a_field_do_not_have_it() {
        let state = ViewState(vec![0x01; 50]);
        assert_eq!(state.mode_id().unwrap(), 0x01);
        assert_eq!(state.zoom(), None);
        assert!(matches!(
            ViewState(vec![0]).status(),
            Err(Error::Protocol(_))
        ));
        assert!(matches!(
            ViewState(Vec::new()).mode_id(),
            Err(Error::Protocol(_))
        ));
    }

    #[test]
    fn a_mode_request_leaves_position_and_zoom_to_the_camera() {
        let payload = ViewState(vec![0x55; 61])
            .mode_request(0x06, Model::Link2)
            .unwrap();
        // The mode, then nothing in the status byte or the bytes after it.
        assert_eq!(payload[0], 0x06);
        assert!(payload[1..38].iter().all(|&byte| byte == 0));
        // Pan, tilt and a third angle, each marked as not set.
        let word = |at: usize| i32::from_le_bytes(payload[at..at + 4].try_into().unwrap());
        assert_eq!([word(38), word(42), word(46)], [3610, 3610, 3610]);
        // No zoom, a host pitch of -45.0 degrees, then zeros.
        assert_eq!(payload[50..52], [0, 0]);
        assert_eq!(payload[52..54], (-450i16).to_le_bytes());
        assert!(payload[54..].iter().all(|&byte| byte == 0));
    }

    #[test]
    fn the_pan_field_depends_on_the_model() {
        let request = |model, len| ViewState(vec![0; len]).mode_request(0x04, model).unwrap();
        // The Link 2C models take a pair of 16-bit values.
        for model in [Model::Link2C, Model::Link2CPro] {
            assert_eq!(request(model, 61)[38..42], [0xff, 0xff, 0xff, 0xff]);
        }
        assert_eq!(request(Model::Link2Pro, 61)[38..42], 3610i32.to_le_bytes());
        // The original Link takes no pan. A payload of 52 bytes ends before the
        // host pitch.
        let link = request(Model::Link, 52);
        assert_eq!(link.len(), 52);
        assert_eq!(link[38..42], [0, 0, 0, 0]);
        assert_eq!(link[42..46], 3610i32.to_le_bytes());
        assert_eq!(link[46..50], 3610i32.to_le_bytes());
        assert_eq!(link[50..52], [0, 0]);
    }
}
