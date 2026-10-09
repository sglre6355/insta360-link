//! Camera settings as one value, for saving and restoring them.
//!
//! [`State`] holds every setting this crate can read and write. It can be read
//! from a camera with [`Camera::state`] and applied to one with
//! [`Camera::apply`].
//!
//! With the `serde` feature, a state can be written to and read from any format
//! `serde` supports. Settings that are absent are left out when writing, and
//! left unchanged when a state read from a file is applied. An unknown key is
//! an error.
//!
//! ```
//! # #[cfg(feature = "serde")] {
//! use insta360_link::{State, WhiteBalance};
//!
//! let state: State = toml::from_str(
//!     r#"
//!     [gimbal]
//!     pan = 20.0
//!
//!     [image.white_balance]
//!     mode = "manual"
//!     temperature = 5000
//!     "#,
//! )?;
//! assert_eq!(state.gimbal.pan, Some(20.0));
//! assert_eq!(
//!     state.image.white_balance,
//!     Some(WhiteBalance::Manual { temperature: 5000 })
//! );
//! # }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::audio::AudioMode;
use crate::camera::Camera;
use crate::error::{Error, Optional, Result};
use crate::features::Features;
use crate::gestures::Gestures;
use crate::gimbal::position::PanTiltUpdate;
use crate::image::settings::ImageSettings;
use crate::mode::CameraMode;
use crate::tracking::framing::{Framing, TrackingSpeed};

/// Whether a group of settings has nothing set, so that it can be left out
/// when writing.
#[cfg(feature = "serde")]
fn is_unset<T: Default + PartialEq>(group: &T) -> bool {
    *group == T::default()
}

/// A snapshot of camera settings or a set of changes to apply.
///
/// All settings are optional. In a value returned by [`Camera::state`], `None`
/// indicates an unsupported or unavailable setting. In a value passed to
/// [`Camera::apply`], it leaves the corresponding setting unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, deny_unknown_fields)
)]
pub struct State {
    /// Camera mode.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub mode: Option<CameraMode>,
    /// Microphone processing mode.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub audio_mode: Option<AudioMode>,
    /// Where the camera points and how far it is zoomed in.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "is_unset"))]
    pub gimbal: GimbalState,
    /// How tracking frames and follows its subject.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "is_unset"))]
    pub tracking: TrackingState,
    /// Picture settings, including exposure.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "is_unset"))]
    pub image: ImageSettings,
    /// Feature switches.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "is_unset"))]
    pub features: Features,
    /// Which hand gestures are enabled.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "is_unset"))]
    pub gestures: Gestures,
}

/// The gimbal part of a [`State`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, deny_unknown_fields)
)]
pub struct GimbalState {
    /// Horizontal gimbal angle in degrees.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub pan: Option<f64>,
    /// Vertical gimbal angle in degrees.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub tilt: Option<f64>,
    /// Zoom factor.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub zoom: Option<f64>,
}

/// The tracking part of a [`State`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, deny_unknown_fields)
)]
pub struct TrackingState {
    /// Smart Composition framing style.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub framing: Option<Framing>,
    /// Tracking speed.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub speed: Option<TrackingSpeed>,
}

impl Camera {
    /// Reads every setting the camera has.
    ///
    /// A gimbal position the camera cannot report plausibly is left out rather
    /// than failing the whole read.
    pub fn state(&self) -> Result<State> {
        let position = match self.position() {
            Ok(position) => Some(position),
            Err(Error::Unsupported | Error::PositionUnknown) => None,
            Err(err) => return Err(err),
        };
        Ok(State {
            mode: match self.mode() {
                Err(Error::Busy) => None,
                mode => mode.optional()?,
            },
            audio_mode: self.audio_mode().optional()?,
            gimbal: GimbalState {
                pan: position.map(|p| p.pan),
                tilt: position.map(|p| p.tilt),
                zoom: self.zoom().optional()?,
            },
            tracking: TrackingState {
                framing: self.framing().optional()?,
                speed: self.tracking_speed().optional()?,
            },
            image: self.image()?,
            features: self.features()?,
            gestures: self.gestures().optional()?.unwrap_or_default(),
        })
    }

    /// Applies all settings in `state` that are `Some`.
    ///
    /// The mode is applied first because switching modes moves the gimbal, while
    /// the gimbal position is applied last. Position changes are skipped if the
    /// resulting mode controls the gimbal itself, such as [`CameraMode::Tracking`],
    /// [`CameraMode::Overhead`], or [`CameraMode::AutoFraming`].
    ///
    /// Stops at the first setting the camera rejects.
    pub fn apply(&self, state: &State) -> Result<()> {
        if let Some(mode) = state.mode {
            self.set_mode(mode)?;
        }
        if let Some(framing) = state.tracking.framing {
            self.set_framing(framing)?;
        }
        if let Some(speed) = state.tracking.speed {
            self.set_tracking_speed(speed)?;
        }
        self.set_features(&state.features)?;
        self.set_gestures(&state.gestures)?;
        if let Some(mode) = state.audio_mode {
            self.set_audio_mode(mode)?;
        }
        self.set_image(&state.image)?;
        if let Some(zoom) = state.gimbal.zoom {
            self.set_zoom(zoom)?;
        }

        if state.gimbal.pan.is_none() && state.gimbal.tilt.is_none() {
            return Ok(());
        }
        let mode = match state.mode {
            Some(mode) => Some(mode),
            None => match self.mode() {
                // Busy for long means tracking is waiting for someone.
                Err(Error::Busy) => Some(CameraMode::Tracking),
                mode => mode.optional()?,
            },
        };
        if matches!(
            mode,
            Some(CameraMode::Tracking | CameraMode::Overhead | CameraMode::AutoFraming)
        ) {
            return Ok(());
        }
        self.set_position(PanTiltUpdate {
            pan: state.gimbal.pan,
            tilt: state.gimbal.tilt,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::selectors::control;
    use crate::device::transport::Control;
    use crate::testing::FakeCamera;

    fn gimbal_camera() -> FakeCamera {
        FakeCamera::new()
            .control(Control::Pan, -522_000, 522_000, 3600, 0)
            .control(Control::Tilt, -324_000, 360_000, 3600, 0)
            .control(Control::Zoom, 100, 400, 1, 100)
            .control(Control::Brightness, 0, 100, 1, 50)
    }

    #[test]
    fn reads_what_the_camera_has() {
        let fake = gimbal_camera();
        fake.set(Control::Pan, 36_000);
        let expected = State {
            gimbal: GimbalState {
                pan: Some(10.0),
                tilt: Some(0.0),
                zoom: Some(1.0),
            },
            image: ImageSettings {
                brightness: Some(50),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(fake.camera().state().unwrap(), expected);
    }

    #[test]
    fn an_unknown_position_is_left_out() {
        let fake = gimbal_camera();
        fake.set(Control::Pan, 36_772_896);
        let gimbal = fake.camera().state().unwrap().gimbal;
        assert_eq!((gimbal.pan, gimbal.tilt), (None, None));
        assert_eq!(gimbal.zoom, Some(1.0));
    }

    #[test]
    fn applies_only_what_is_present_with_position_last() {
        let fake = gimbal_camera();
        let state = State {
            gimbal: GimbalState {
                pan: None,
                tilt: Some(5.0),
                zoom: Some(2.0),
            },
            image: ImageSettings {
                brightness: Some(60),
                ..Default::default()
            },
            ..Default::default()
        };
        fake.camera().apply(&state).unwrap();
        assert_eq!(
            fake.writes(),
            [
                vec![(Control::Brightness, 60)],
                vec![(Control::Zoom, 200)],
                vec![(Control::Pan, 0), (Control::Tilt, 18_000)],
            ]
        );
        fake.camera().apply(&State::default()).unwrap();
        assert_eq!(fake.writes().len(), 3);
    }

    #[test]
    fn position_is_skipped_in_modes_that_aim_the_gimbal() {
        // Byte 0 of the view state is 1 while tracking.
        let fake = gimbal_camera().selector(control::VIEW_STATE, &[1, 0]);
        let state = State {
            gimbal: GimbalState {
                pan: Some(30.0),
                tilt: None,
                zoom: Some(2.0),
            },
            ..Default::default()
        };
        fake.camera().apply(&state).unwrap();
        assert_eq!(fake.writes(), [[(Control::Zoom, 200)]]);
    }
}

#[cfg(all(test, feature = "serde"))]
mod serde_tests {
    use indoc::indoc;

    use super::*;
    use crate::image::exposure::Exposure;
    use crate::image::settings::{Focus, PowerLine, WhiteBalance};

    /// A state with every setting present.
    fn full() -> State {
        State {
            mode: Some(CameraMode::DeskView),
            audio_mode: Some(AudioMode::VoiceFocus),
            gimbal: GimbalState {
                pan: Some(20.0),
                tilt: Some(-7.4),
                zoom: Some(1.5),
            },
            tracking: TrackingState {
                framing: Some(Framing::HalfBody),
                speed: Some(TrackingSpeed::Fast),
            },
            image: ImageSettings {
                brightness: Some(50),
                contrast: Some(40),
                saturation: Some(60),
                sharpness: Some(50),
                hue: Some(-3),
                white_balance: Some(WhiteBalance::Manual { temperature: 5000 }),
                focus: Some(Focus::Auto),
                power_line: Some(PowerLine::Hz50),
                exposure: Some(Exposure::Manual {
                    iso: 400,
                    shutter: 50,
                }),
            },
            features: Features {
                ai_zoom: Some(true),
                hdr: Some(true),
                horizontal_flip: Some(false),
                vertical_flip: Some(false),
                gestures: Some(true),
                smart_adjustment: Some(false),
                mute_in_privacy: Some(false),
                noise_canceling: Some(true),
                horizontal_correction: Some(false),
            },
            gestures: Gestures {
                tracking: Some(true),
                zoom: Some(false),
                whiteboard: Some(true),
            },
        }
    }

    const FULL_TOML: &str = indoc! {r#"
        mode = "deskview"
        audio_mode = "voice-focus"

        [gimbal]
        pan = 20.0
        tilt = -7.4
        zoom = 1.5

        [tracking]
        framing = "half-body"
        speed = "fast"

        [image]
        brightness = 50
        contrast = 40
        saturation = 60
        sharpness = 50
        hue = -3
        power_line = "50"

        [image.white_balance]
        mode = "manual"
        temperature = 5000

        [image.focus]
        mode = "auto"

        [image.exposure]
        mode = "manual"
        iso = 400
        shutter = 50

        [features]
        ai_zoom = true
        hdr = true
        horizontal_flip = false
        vertical_flip = false
        gestures = true
        smart_adjustment = false
        mute_in_privacy = false
        noise_canceling = true
        horizontal_correction = false

        [gestures]
        tracking = true
        zoom = false
        whiteboard = true
    "#};

    #[test]
    fn toml_round_trips() {
        assert_eq!(toml::to_string(&full()).unwrap(), FULL_TOML);
        assert_eq!(toml::from_str::<State>(FULL_TOML).unwrap(), full());
    }

    #[test]
    fn json_round_trips() {
        let json = serde_json::to_string(&full()).unwrap();
        assert_eq!(serde_json::from_str::<State>(&json).unwrap(), full());
        assert!(json.contains(r#""mode":"deskview""#));
        assert!(json.contains(r#""white_balance":{"mode":"manual","temperature":5000}"#));
        assert!(json.contains(r#""focus":{"mode":"auto"}"#));
    }

    #[test]
    fn absent_settings_are_left_out_and_read_as_none() {
        assert_eq!(toml::to_string(&State::default()).unwrap(), "");
        assert_eq!(serde_json::to_string(&State::default()).unwrap(), "{}");
        let partial: State = toml::from_str("[gimbal]\npan = 20.0\n").unwrap();
        assert_eq!(partial.gimbal.pan, Some(20.0));
        assert_eq!(partial.gimbal.tilt, None);
        assert_eq!(partial.image, ImageSettings::default());
        assert_eq!(toml::to_string(&partial).unwrap(), "[gimbal]\npan = 20.0\n");
    }

    #[test]
    fn a_mode_selects_which_values_apply() {
        let state: State = toml::from_str(indoc! {r#"
            [image]
            power_line = "off"

            [image.white_balance]
            mode = "auto"

            [image.focus]
            mode = "manual"
            position = 30

            [image.exposure]
            mode = "auto"
            compensation = -1.5
        "#})
        .unwrap();
        assert_eq!(state.image.white_balance, Some(WhiteBalance::Auto));
        assert_eq!(state.image.focus, Some(Focus::Manual { position: 30 }));
        assert_eq!(state.image.power_line, Some(PowerLine::Off));
        assert_eq!(
            state.image.exposure,
            Some(Exposure::Auto { compensation: -1.5 })
        );
    }

    #[test]
    fn rejects_values_that_belong_to_the_other_mode() {
        for text in [
            "[image.white_balance]\nmode = \"auto\"\ntemperature = 5000\n",
            "[image.white_balance]\nmode = \"manual\"\n",
            "[image.white_balance]\ntemperature = 5000\n",
            "[image.focus]\nmode = \"auto\"\nposition = 30\n",
            "[image.exposure]\nmode = \"auto\"\niso = 400\n",
            "[image.exposure]\nmode = \"manual\"\ncompensation = 1.0\n",
            "[image.exposure]\niso = 400\n",
            "[image.exposure]\nmode = \"manual\"\n",
            "[image.exposure]\nmode = \"manual\"\niso = 400\n",
            "[image.exposure]\nmode = \"auto\"\n",
        ] {
            assert!(toml::from_str::<State>(text).is_err(), "{text}");
        }
    }

    #[test]
    fn rejects_unknown_keys_and_values() {
        for text in [
            "volume = 3\n",
            "[gimbal]\nroll = 1.0\n",
            "mode = \"tracking\"\n",
            "[image]\nwhite_balance = 5000\n",
            "[image.white_balance]\nmode = \"warm\"\n",
            "[features]\nhdr = \"on\"\n",
        ] {
            assert!(toml::from_str::<State>(text).is_err(), "{text}");
        }
    }
}
