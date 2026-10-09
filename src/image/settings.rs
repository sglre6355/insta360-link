//! Image settings: color, white balance, focus, anti-flicker, and exposure.
//!
//! All except exposure use standard UVC processing controls. Each setting is
//! written separately because the camera rejects manual values sent in the same
//! request as disabling automatic mode.

use std::fmt;
use std::str::FromStr;

use crate::camera::Camera;
use crate::device::transport::Control;
use crate::error::{Error, Optional, ParseSettingError, Result};
use crate::image::exposure::Exposure;

/// White balance: automatic, or a fixed color temperature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(
        tag = "mode",
        rename_all = "kebab-case",
        try_from = "config::WhiteBalance"
    )
)]
pub enum WhiteBalance {
    /// The camera picks the color temperature.
    Auto,
    /// A fixed color temperature.
    Manual {
        /// The color temperature in kelvin.
        temperature: u32,
    },
}

/// Focus: continuous autofocus, or a fixed distance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(tag = "mode", rename_all = "kebab-case", try_from = "config::Focus")
)]
pub enum Focus {
    /// Continuous autofocus.
    Auto,
    /// A fixed focus distance.
    Manual {
        /// The focus position on the camera's own scale, usually 0 to 100.
        position: u32,
    },
}

/// Compensation for the flicker of mains-powered lighting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "kebab-case")
)]
pub enum PowerLine {
    /// No compensation.
    Off,
    /// 50 Hz mains.
    #[cfg_attr(feature = "serde", serde(rename = "50"))]
    Hz50,
    /// 60 Hz mains.
    #[cfg_attr(feature = "serde", serde(rename = "60"))]
    Hz60,
    /// The camera detects the mains frequency.
    Auto,
}

/// A set of image settings.
///
/// This type is used for both reading and writing. In a value returned by
/// [`Camera::image`], `None` indicates an unsupported setting. In a value
/// passed to [`Camera::set_image`], it leaves the corresponding setting
/// unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default, deny_unknown_fields)
)]
pub struct ImageSettings {
    /// Brightness, usually 0 to 100.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub brightness: Option<i32>,
    /// Contrast, usually 0 to 100.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub contrast: Option<i32>,
    /// Saturation, usually 0 to 100.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub saturation: Option<i32>,
    /// Sharpness, usually 0 to 100.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub sharpness: Option<i32>,
    /// Hue, usually -15 to 15.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub hue: Option<i32>,
    /// White balance.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub white_balance: Option<WhiteBalance>,
    /// Focus.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub focus: Option<Focus>,
    /// Anti-flicker.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub power_line: Option<PowerLine>,
    /// Exposure.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub exposure: Option<Exposure>,
}

/// The plain integer settings: field name, control, and accessors.
type Simple = (
    &'static str,
    Control,
    fn(&ImageSettings) -> Option<i32>,
    fn(&mut ImageSettings, i32),
);

const SIMPLE: [Simple; 5] = [
    (
        "brightness",
        Control::Brightness,
        |s| s.brightness,
        |s, v| s.brightness = Some(v),
    ),
    (
        "contrast",
        Control::Contrast,
        |s| s.contrast,
        |s, v| s.contrast = Some(v),
    ),
    (
        "saturation",
        Control::Saturation,
        |s| s.saturation,
        |s, v| s.saturation = Some(v),
    ),
    (
        "sharpness",
        Control::Sharpness,
        |s| s.sharpness,
        |s, v| s.sharpness = Some(v),
    ),
    ("hue", Control::Hue, |s| s.hue, |s, v| s.hue = Some(v)),
];

impl Camera {
    /// Reads the current picture settings.
    pub fn image(&self) -> Result<ImageSettings> {
        let mut settings = ImageSettings::default();
        for (_, id, _, set) in SIMPLE {
            if let Some(value) = self.get_control(id).optional()? {
                set(&mut settings, value);
            }
        }

        settings.white_balance = match self.get_control(Control::AutoWhiteBalance).optional()? {
            None => None,
            Some(0) => {
                let kelvin = self.get_control(Control::WhiteBalanceTemperature)?;
                Some(WhiteBalance::Manual {
                    temperature: kelvin.max(0) as u32,
                })
            }
            Some(_) => Some(WhiteBalance::Auto),
        };

        settings.focus = match self.get_control(Control::AutoFocus).optional()? {
            None => None,
            Some(0) => {
                let position = self.get_control(Control::Focus)?;
                Some(Focus::Manual {
                    position: position.max(0) as u32,
                })
            }
            Some(_) => Some(Focus::Auto),
        };

        settings.power_line = self
            .get_control(Control::PowerLineFrequency)
            .optional()?
            .and_then(|raw| match raw {
                0 => Some(PowerLine::Off),
                1 => Some(PowerLine::Hz50),
                2 => Some(PowerLine::Hz60),
                3 => Some(PowerLine::Auto),
                _ => None,
            });

        settings.exposure = self.exposure().optional()?;
        Ok(settings)
    }

    /// Updates the image settings set to `Some`, leaving the others unchanged.
    ///
    /// Setting a fixed white balance or focus first disables the corresponding
    /// automatic mode. All values are validated against the camera's reported
    /// ranges before any changes are made.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::NotStreaming`] if exposure settings are specified
    /// but video is not streaming. Other settings do not require streaming.
    pub fn set_image(&self, settings: &ImageSettings) -> Result<()> {
        let mut writes = Vec::new();
        for (name, id, get, _) in SIMPLE {
            if let Some(value) = get(settings) {
                writes.push((id, self.checked(id, name, value.into())?));
            }
        }
        match settings.white_balance {
            None => {}
            Some(WhiteBalance::Auto) => {
                self.control_info(Control::AutoWhiteBalance)?;
                writes.push((Control::AutoWhiteBalance, 1));
            }
            Some(WhiteBalance::Manual {
                temperature: kelvin,
            }) => {
                let raw = self.checked(
                    Control::WhiteBalanceTemperature,
                    "white balance",
                    kelvin.into(),
                )?;
                writes.push((Control::AutoWhiteBalance, 0));
                writes.push((Control::WhiteBalanceTemperature, raw));
            }
        }
        match settings.focus {
            None => {}
            Some(Focus::Auto) => {
                self.control_info(Control::AutoFocus)?;
                writes.push((Control::AutoFocus, 1));
            }
            Some(Focus::Manual { position }) => {
                let raw = self.checked(Control::Focus, "focus", position.into())?;
                writes.push((Control::AutoFocus, 0));
                writes.push((Control::Focus, raw));
            }
        }
        if let Some(power_line) = settings.power_line {
            // No range check: the camera accepts "auto" (3) while reporting a
            // maximum of 2.
            self.control_info(Control::PowerLineFrequency)?;
            let raw = match power_line {
                PowerLine::Off => 0,
                PowerLine::Hz50 => 1,
                PowerLine::Hz60 => 2,
                PowerLine::Auto => 3,
            };
            writes.push((Control::PowerLineFrequency, raw));
        }

        if let Some(exposure) = &settings.exposure {
            self.set_exposure(exposure)?;
        }
        for (id, value) in writes {
            self.set_control(id, value)?;
        }
        Ok(())
    }

    /// Checks a value against a control's reported range.
    fn checked(&self, id: Control, what: &'static str, value: i64) -> Result<i32> {
        let info = self.control_info(id)?;
        if (i64::from(info.minimum)..=i64::from(info.maximum)).contains(&value) {
            Ok(value as i32)
        } else {
            Err(Error::OutOfRange {
                what,
                value: value as f64,
                min: info.minimum.into(),
                max: info.maximum.into(),
            })
        }
    }
}

impl fmt::Display for WhiteBalance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => f.write_str("auto"),
            Self::Manual { temperature } => write!(f, "{temperature}"),
        }
    }
}

impl FromStr for WhiteBalance {
    type Err = ParseSettingError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Self::Auto),
            _ => s
                .parse()
                .map(|temperature| Self::Manual { temperature })
                .map_err(|_| ParseSettingError::new("`auto` or a temperature in kelvin", s)),
        }
    }
}

impl fmt::Display for Focus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => f.write_str("auto"),
            Self::Manual { position } => write!(f, "{position}"),
        }
    }
}

impl FromStr for Focus {
    type Err = ParseSettingError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Self::Auto),
            _ => s
                .parse()
                .map(|position| Self::Manual { position })
                .map_err(|_| ParseSettingError::new("`auto` or a focus position", s)),
        }
    }
}

impl fmt::Display for PowerLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Off => "off",
            Self::Hz50 => "50",
            Self::Hz60 => "60",
            Self::Auto => "auto",
        })
    }
}

impl FromStr for PowerLine {
    type Err = ParseSettingError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "off" => Ok(Self::Off),
            "50" => Ok(Self::Hz50),
            "60" => Ok(Self::Hz60),
            "auto" => Ok(Self::Auto),
            _ => Err(ParseSettingError::new("`off`, `50`, `60` or `auto`", s)),
        }
    }
}

/// How white balance and focus are read from a config. A value that belongs
/// to the other mode is an error, which the derived reader would let through
/// for the automatic mode.
#[cfg(feature = "serde")]
mod config {
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(rename_all = "kebab-case")]
    enum Mode {
        Auto,
        Manual,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct WhiteBalance {
        mode: Mode,
        temperature: Option<u32>,
    }

    impl TryFrom<WhiteBalance> for super::WhiteBalance {
        type Error = &'static str;

        fn try_from(config: WhiteBalance) -> Result<Self, Self::Error> {
            match (config.mode, config.temperature) {
                (Mode::Auto, None) => Ok(Self::Auto),
                (Mode::Manual, Some(temperature)) => Ok(Self::Manual { temperature }),
                (Mode::Auto, Some(_)) => Err("`temperature` only applies in manual mode"),
                (Mode::Manual, None) => Err("manual mode needs a `temperature`"),
            }
        }
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Focus {
        mode: Mode,
        position: Option<u32>,
    }

    impl TryFrom<Focus> for super::Focus {
        type Error = &'static str;

        fn try_from(config: Focus) -> Result<Self, Self::Error> {
            match (config.mode, config.position) {
                (Mode::Auto, None) => Ok(Self::Auto),
                (Mode::Manual, Some(position)) => Ok(Self::Manual { position }),
                (Mode::Auto, Some(_)) => Err("`position` only applies in manual mode"),
                (Mode::Manual, None) => Err("manual mode needs a `position`"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::selectors::control;
    use crate::testing::FakeCamera;

    fn image_camera() -> FakeCamera {
        FakeCamera::new()
            .control(Control::Brightness, 0, 100, 1, 50)
            .control(Control::Contrast, 0, 100, 1, 50)
            .control(Control::Saturation, 0, 100, 1, 50)
            .control(Control::Sharpness, 0, 100, 1, 50)
            .control(Control::Hue, -15, 15, 1, 0)
            .control(Control::AutoWhiteBalance, 0, 1, 1, 1)
            .control(Control::WhiteBalanceTemperature, 2000, 10000, 1, 6400)
            .control(Control::AutoFocus, 0, 1, 1, 1)
            .control(Control::Focus, 0, 100, 1, 50)
            .control(Control::PowerLineFrequency, 0, 3, 1, 3)
    }

    #[test]
    fn reads_defaults() {
        let settings = image_camera().camera().image().unwrap();
        assert_eq!(
            settings,
            ImageSettings {
                brightness: Some(50),
                contrast: Some(50),
                saturation: Some(50),
                sharpness: Some(50),
                hue: Some(0),
                white_balance: Some(WhiteBalance::Auto),
                focus: Some(Focus::Auto),
                power_line: Some(PowerLine::Auto),
                exposure: None,
            }
        );
    }

    /// The same camera with automatic exposure, streaming or not.
    fn exposure_camera(streaming: bool) -> FakeCamera {
        image_camera()
            .selector(control::EXPOSURE_MODE, &[2])
            .selector(control::EXPOSURE_COMPENSATION, &[0, 0])
            .selector(control::DEVICE_STATUS, &[0, 0, 0, u8::from(streaming), 0])
    }

    #[test]
    fn exposure_is_read_and_written_with_the_rest() {
        let fake = exposure_camera(true);
        let camera = fake.camera();
        assert_eq!(
            camera.image().unwrap().exposure,
            Some(Exposure::Auto { compensation: 0.0 })
        );

        let brighter = ImageSettings {
            brightness: Some(60),
            exposure: Some(Exposure::Auto { compensation: 1.0 }),
            ..Default::default()
        };
        camera.set_image(&brighter).unwrap();
        assert_eq!(
            fake.xu_writes(),
            [(9, control::EXPOSURE_COMPENSATION.id, vec![100, 0])]
        );
        assert_eq!(fake.get(Control::Brightness), 60);
    }

    #[test]
    fn only_exposure_settings_need_a_stream() {
        let fake = exposure_camera(false);
        let camera = fake.camera();
        let brightness_only = ImageSettings {
            brightness: Some(60),
            ..Default::default()
        };
        camera.set_image(&brightness_only).unwrap();

        let with_exposure = ImageSettings {
            brightness: Some(70),
            exposure: Some(Exposure::Auto { compensation: 1.0 }),
            ..Default::default()
        };
        assert!(matches!(
            camera.set_image(&with_exposure),
            Err(Error::NotStreaming)
        ));
        // The refused change left the other settings alone too.
        assert_eq!(fake.get(Control::Brightness), 60);
    }

    #[test]
    fn missing_controls_read_as_none() {
        let fake = FakeCamera::new().control(Control::Brightness, 0, 100, 1, 50);
        let settings = fake.camera().image().unwrap();
        assert_eq!(
            settings,
            ImageSettings {
                brightness: Some(50),
                ..Default::default()
            }
        );
    }

    #[test]
    fn only_given_settings_are_written() {
        let fake = image_camera();
        fake.camera()
            .set_image(&ImageSettings {
                contrast: Some(70),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(fake.writes(), [[(Control::Contrast, 70)]]);
    }

    #[test]
    fn manual_values_follow_the_auto_switch_in_separate_requests() {
        let fake = image_camera();
        let camera = fake.camera();
        camera
            .set_image(&ImageSettings {
                white_balance: Some(WhiteBalance::Manual { temperature: 5000 }),
                focus: Some(Focus::Manual { position: 30 }),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            fake.writes(),
            [
                [(Control::AutoWhiteBalance, 0)],
                [(Control::WhiteBalanceTemperature, 5000)],
                [(Control::AutoFocus, 0)],
                [(Control::Focus, 30)],
            ]
        );
        let settings = camera.image().unwrap();
        assert_eq!(
            settings.white_balance,
            Some(WhiteBalance::Manual { temperature: 5000 })
        );
        assert_eq!(settings.focus, Some(Focus::Manual { position: 30 }));

        camera
            .set_image(&ImageSettings {
                white_balance: Some(WhiteBalance::Auto),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            camera.image().unwrap().white_balance,
            Some(WhiteBalance::Auto)
        );
    }

    #[test]
    fn nothing_is_written_if_any_value_is_out_of_range() {
        let fake = image_camera();
        let err = fake
            .camera()
            .set_image(&ImageSettings {
                brightness: Some(60),
                white_balance: Some(WhiteBalance::Manual { temperature: 1000 }),
                ..Default::default()
            })
            .unwrap_err();
        assert!(matches!(
            err,
            Error::OutOfRange { what: "white balance", min, max, .. }
                if min == 2000.0 && max == 10000.0
        ));
        assert!(fake.writes().is_empty());
    }

    #[test]
    fn setting_a_missing_control_is_unsupported() {
        let fake = FakeCamera::new().control(Control::Brightness, 0, 100, 1, 50);
        let err = fake
            .camera()
            .set_image(&ImageSettings {
                focus: Some(Focus::Auto),
                ..Default::default()
            })
            .unwrap_err();
        assert!(matches!(err, Error::Unsupported));
    }

    #[test]
    fn text_forms_round_trip() {
        for text in ["auto", "5000"] {
            assert_eq!(text.parse::<WhiteBalance>().unwrap().to_string(), text);
            assert_eq!(text.parse::<Focus>().unwrap().to_string(), text);
        }
        for text in ["off", "50", "60", "auto"] {
            assert_eq!(text.parse::<PowerLine>().unwrap().to_string(), text);
        }
        assert!("warm".parse::<WhiteBalance>().is_err());
        assert!("-1".parse::<Focus>().is_err());
        assert_eq!(
            "70".parse::<PowerLine>().unwrap_err().to_string(),
            "expected `off`, `50`, `60` or `auto`, got `70`"
        );
    }
}
