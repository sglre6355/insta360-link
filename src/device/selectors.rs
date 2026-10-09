//! Known selectors of the three Link vendor extension units.
//!
//! Each selector identifies a numbered control containing a small byte payload.
//! This module defines the selectors, while their payload formats are
//! documented in the modules that use them.
//!
//! The control unit's selector list is complete. Selectors used by this crate
//! have descriptive names, with their vendor names noted alongside them. Unused
//! selectors retain their vendor names, joined with `_OR_` when multiple names
//! refer to the same selector. For the other two units, only selectors used by
//! this crate are listed.

use std::fmt;

use super::xu::UnitKind;

/// The address of a selector: a unit and a number within it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Selector {
    pub(crate) unit: UnitKind,
    pub(crate) id: u8,
}

impl Selector {
    pub(crate) const fn new(unit: UnitKind, id: u8) -> Self {
        Self { unit, id }
    }

    fn entry(self) -> Option<&'static Entry> {
        [control::ENTRIES, aux::ENTRIES, ai::ENTRIES]
            .into_iter()
            .flatten()
            .find(|entry| entry.selector == self)
    }

    /// The selector's name in lowercase, if it is a known one.
    pub(crate) fn name(self) -> Option<String> {
        self.entry().map(|entry| entry.name.to_ascii_lowercase())
    }

    /// Whether this crate refuses to touch the selector.
    pub(crate) fn is_blocked(self) -> bool {
        self.entry().is_some_and(|entry| entry.blocked)
    }
}

impl fmt::Display for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} unit selector {}", self.unit, self.id)
    }
}

struct Entry {
    selector: Selector,
    name: &'static str,
    blocked: bool,
}

/// Declares the selectors of one unit as constants, and collects them into the
/// table that names them and marks the blocked ones.
macro_rules! selectors {
    ($module:ident, $unit:expr, [$($(#[$doc:meta])* $name:ident = $id:literal $($blocked:ident)?,)*]) => {
        pub(crate) mod $module {
            use super::{Entry, Selector};
            use crate::device::xu::UnitKind;

            $(
                $(#[$doc])*
                pub(crate) const $name: Selector = Selector::new($unit, $id);
            )*

            pub(super) const ENTRIES: &[Entry] = &[$(
                Entry {
                    selector: $name,
                    name: stringify!($name),
                    blocked: selectors!(@blocked $($blocked)?),
                },
            )*];
        }
    };
    (@blocked) => { false };
    (@blocked blocked) => { true };
}

selectors!(control, UnitKind::Control, [
    EXEC_SCRIPT = 1 blocked,
    /// Camera mode in bytes 0 and 1. The longer, 61-byte payload used by newer
    /// cameras also reports the actual pan, tilt, and zoom, and whether privacy
    /// mode is on. See [`ViewState`](crate::device::view_state::ViewState).
    /// Vendor name: `video_mode`.
    VIEW_STATE = 2,
    /// Device information, including the firmware version.
    DEVICE_INFO = 3,
    PTZ_CMD = 4,
    /// Per-gesture bitmask, one byte.
    /// Vendor name: `gesture_status`.
    GESTURES = 5,
    GESTURE_BIND = 6,
    /// Microphone noise cancellation, one byte.
    /// Vendor name: `noise_cancel`.
    NOISE_CANCELING = 7,
    FIRMWARE_UPGRADE_OR_BLEND_DRAW = 8 blocked,
    /// Exposure compensation in hundredths of an EV stop, stored as a
    /// little-endian `i16`.
    /// Vendor name: `exposure_value`.
    EXPOSURE_COMPENSATION = 9,
    TAKE_PICTURE = 10,
    /// Device status. Byte 3 is nonzero while video is streaming.
    DEVICE_STATUS = 11,
    /// Serial number as text.
    /// Vendor name: `device_sn`.
    SERIAL_NUMBER = 12,
    DEVICE_LICENSE = 13 blocked,
    DEVICE_PARAM = 14,
    AF_MODE_OR_DOWNLOAD_FILE = 15 blocked,
    EXPOSURE_CURVE_OR_UPLOAD_FILE = 16 blocked,
    USB_MODE_SWITCH = 17 blocked,
    /// Tracking speed, one byte.
    /// Vendor name: `track_speed`.
    TRACKING_SPEED = 18,
    /// Tracking framing, one byte.
    /// Vendor name: `layout_style`.
    FRAMING = 19,
    HEAD_LIST = 20,
    TRACK_TARGET = 21,
    PANTILT_RELATIVE = 22,
    MOBVOI_PUBKEY = 23 blocked,
    BIAS = 24,
    /// ISO as a little-endian `u16`. In automatic mode, reports the value
    /// currently used by the camera.
    ISO = 25,
    /// Actual gimbal position in arcseconds, stored as two little-endian `i32`
    /// values, with tilt first.
    /// Vendor name: `pantilt_absolute`.
    MEASURED_POSITION = 26,
    /// Feature bitmask, stored as a little-endian `u16`.
    /// Vendor name: `func_enable`.
    FEATURES = 27,
    VIDEO_RES = 28,
    /// Shutter time as the denominator of a fraction of a second, stored as a
    /// little-endian `u16`. In automatic mode, reports the current value.
    /// Vendor name: `exposure_time_absolute`.
    SHUTTER = 29,
    /// Auto-exposure mode, one byte.
    /// Vendor name: `ae_mode`.
    EXPOSURE_MODE = 30,
]);

selectors!(
    aux,
    UnitKind::Aux,
    [
        /// Privacy switch, two bytes. Reads return the same value regardless of
        /// whether privacy mode is enabled. The actual state is reported
        /// through the view state.
        PRIVACY = 15,
        /// Audio mode, one byte.
        AUDIO_MODE = 20,
        /// Six pause-track zone records, 15 bytes each.
        PAUSE_AREAS = 21,
        /// Horizontal correction in the first byte of a 25-byte payload.
        HORIZONTAL_CORRECTION = 28,
    ]
);

selectors!(
    ai,
    UnitKind::Ai,
    [
        /// Tracking switch, one byte.
        TRACKING = 2,
        /// Preset record, 127 bytes. Writing a record with its slot byte set to
        /// the slot number plus 100 selects the slot returned by the next read.
        PRESET_RECORD = 28,
        /// Tracking area, 19 bytes containing two flags, eight little-endian
        /// `i16` angles in tenths of a degree, and a final byte.
        TRACKING_AREA = 31,
    ]
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_constants() {
        assert_eq!(control::VIEW_STATE.name().as_deref(), Some("view_state"));
        assert_eq!(aux::PRIVACY.name().as_deref(), Some("privacy"));
        assert_eq!(ai::TRACKING.name().as_deref(), Some("tracking"));
        assert_eq!(Selector::new(UnitKind::Control, 31).name(), None);
        assert_eq!(Selector::new(UnitKind::Aux, 2).name(), None);
    }

    #[test]
    fn dangerous_selectors_are_blocked() {
        for id in [1, 8, 13, 15, 16, 17, 23] {
            assert!(Selector::new(UnitKind::Control, id).is_blocked());
        }
        assert!(!control::VIEW_STATE.is_blocked());
        // The same numbers in other units are different selectors.
        assert!(!aux::PRIVACY.is_blocked());
        assert!(!Selector::new(UnitKind::Ai, 1).is_blocked());
    }

    #[test]
    fn no_selector_is_listed_twice() {
        for entries in [control::ENTRIES, aux::ENTRIES, ai::ENTRIES] {
            for (index, entry) in entries.iter().enumerate() {
                assert!(
                    entries[..index]
                        .iter()
                        .all(|other| other.selector != entry.selector),
                    "{} is listed twice",
                    entry.selector
                );
            }
        }
    }
}
