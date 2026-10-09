//! Control Insta360 Link series webcams.
//!
//! This crate works alongside the operating system's own camera driver. It does
//! not take the camera away from it, so settings can be changed while another
//! program is streaming video.
//!
//! It is unofficial and not affiliated with Insta360. The protocol was worked
//! out from Insta360's Link Controller application and by observing a camera.
//! See the README for which models and features have been verified on real
//! hardware.
//!
//! ```no_run
//! use insta360_link::Camera;
//!
//! let camera = Camera::open_first()?;
//! println!("{} at {}", camera.info().model, camera.info().id);
//! # Ok::<(), insta360_link::Error>(())
//! ```

#![warn(missing_docs)]

mod audio;
mod camera;
mod device;
mod error;
mod features;
mod gestures;
mod gimbal;
mod image;
mod mode;
mod model;
mod state;
#[cfg(test)]
mod testing;
mod tracking;

pub use audio::AudioMode;
pub use camera::Camera;
pub use device::info::{DeviceId, DeviceInfo};
pub use device::probe::{ProbeValue, SelectorProbe, XuInfo};
pub use device::{transport, xu};
pub use error::{Error, ParseSettingError, Result};
pub use features::Features;
pub use gestures::Gestures;
pub use gimbal::position::{PanTilt, PanTiltLimits, PanTiltUpdate};
pub use gimbal::presets::{PRESET_NAME_MAX, PRESET_SLOTS, Preset};
pub use image::exposure::Exposure;
pub use image::settings::{Focus, ImageSettings, PowerLine, WhiteBalance};
pub use mode::{CameraMode, ModeStatus};
pub use model::{Model, VENDOR_ID};
pub use state::{GimbalState, State, TrackingState};
pub use tracking::framing::{Framing, TrackingSpeed};
pub use tracking::pause_areas::{PAUSE_AREA_SLOTS, PauseArea};
pub use tracking::tracking_area::TrackingArea;
