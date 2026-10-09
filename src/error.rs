//! The crate's error type.

use std::io;

use crate::device::info::DeviceId;
use crate::device::xu::UnitKind;

/// A specialized `Result` for camera operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong while finding or controlling a camera.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// No Link camera is connected.
    #[error("no Insta360 Link series camera found")]
    NotFound,

    /// The given address exists but does not belong to a Link camera.
    /// On Linux it must be the device node that carries the camera's controls.
    #[error("{0} is not an Insta360 Link series camera")]
    NotALink(DeviceId),

    /// The current user may not open the camera.
    #[error("permission denied opening {0}")]
    PermissionDenied(DeviceId),

    /// The camera does not have what the operation needs.
    #[error("not supported by this camera")]
    Unsupported,

    /// The selector is one this crate refuses to touch.
    /// See [`is_blocked`](crate::xu::is_blocked).
    #[error("selector {selector} of the {unit} unit is blocked: it can damage the camera")]
    Blocked {
        /// The unit the selector belongs to.
        unit: UnitKind,
        /// The refused selector.
        selector: u8,
    },

    /// A value lies outside what the camera accepts.
    #[error("{what} {value} is out of range ({min} to {max})")]
    OutOfRange {
        /// The setting being changed.
        what: &'static str,
        /// The rejected value.
        value: f64,
        /// Smallest accepted value.
        min: f64,
        /// Largest accepted value.
        max: f64,
    },

    /// The camera reported a pan or tilt position outside its valid range,
    /// preventing changes to one axis while preserving the other. Both axes can
    /// still be set together.
    #[error("the camera reports an invalid position; set both pan and tilt")]
    PositionUnknown,

    /// The lower limit of a range is not below its upper limit.
    #[error("{what}: {min} is not below {max}")]
    InvalidRange {
        /// What the range limits.
        what: &'static str,
        /// The lower limit given.
        min: f64,
        /// The upper limit given.
        max: f64,
    },

    /// The preset slot holds nothing.
    #[error("preset slot {slot} is empty")]
    EmptyPreset {
        /// The slot that was asked for.
        slot: u8,
    },

    /// The camera only applies this change while video is streaming, and no
    /// program is streaming from it.
    #[error("this can only be changed while the camera is streaming video")]
    NotStreaming,

    /// The camera is between modes and cannot say which one it is in. This
    /// lasts a second or so during a mode change, and for as long as tracking
    /// has no one to follow.
    #[error("the camera is busy changing mode")]
    Busy,

    /// The camera accepted a change but did not apply it in time.
    #[error("the camera did not apply the change")]
    NotApplied,

    /// The camera answered with data this crate does not understand.
    #[error("unexpected response from the camera: {0}")]
    Protocol(String),

    /// The operating system or the camera rejected a request.
    #[error("{context}: {source}")]
    Io {
        /// What was being attempted.
        context: String,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
}

impl Error {
    pub(crate) fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

/// Lets a caller treat something the camera lacks as having no value.
pub(crate) trait Optional<T> {
    /// Turns [`Error::Unsupported`] into `None` and keeps every other outcome.
    fn optional(self) -> Result<Option<T>>;
}

impl<T> Optional<T> for Result<T> {
    fn optional(self) -> Result<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(Error::Unsupported) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

/// The error for a setting written in text that could not be understood.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("expected {expected}, got `{input}`")]
pub struct ParseSettingError {
    expected: &'static str,
    input: String,
}

impl ParseSettingError {
    pub(crate) fn new(expected: &'static str, input: &str) -> Self {
        Self {
            expected,
            input: input.to_owned(),
        }
    }
}
