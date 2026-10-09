//! How tracking frames its subject and how fast it follows.

use std::fmt;
use std::str::FromStr;

use crate::camera::Camera;
use crate::device::selectors::control;
use crate::error::{Error, ParseSettingError, Result};

/// How Smart Composition frames the person being tracked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "kebab-case")
)]
#[non_exhaustive]
pub enum Framing {
    /// Head and shoulders.
    Head,
    /// Upper body.
    HalfBody,
    /// The whole person.
    WholeBody,
    /// No reframing: the zoom stays where it was put.
    ///
    /// Shown in Link Controller with Smart Composition disabled.
    /// Automatically activated when zoom is adjusted manually.
    Manual,
}

impl Framing {
    const ALL: [Self; 4] = [Self::Head, Self::HalfBody, Self::WholeBody, Self::Manual];

    const fn code(self) -> u8 {
        match self {
            Self::Head => 1,
            Self::HalfBody => 2,
            Self::WholeBody => 3,
            Self::Manual => 4,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Head => "head",
            Self::HalfBody => "half-body",
            Self::WholeBody => "whole-body",
            Self::Manual => "manual",
        }
    }
}

impl fmt::Display for Framing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Framing {
    type Err = ParseSettingError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|framing| framing.name() == s)
            .ok_or_else(|| {
                ParseSettingError::new("`head`, `half-body`, `whole-body` or `manual`", s)
            })
    }
}

/// How fast tracking follows its subject.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "kebab-case")
)]
pub enum TrackingSpeed {
    /// Follows gently.
    Slow,
    /// The default.
    Normal,
    /// Follows quickly.
    Fast,
}

impl TrackingSpeed {
    const ALL: [Self; 3] = [Self::Slow, Self::Normal, Self::Fast];

    const fn code(self) -> u8 {
        match self {
            Self::Slow => 2,
            Self::Normal => 1,
            Self::Fast => 0,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Slow => "slow",
            Self::Normal => "normal",
            Self::Fast => "fast",
        }
    }
}

impl fmt::Display for TrackingSpeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for TrackingSpeed {
    type Err = ParseSettingError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|speed| speed.name() == s)
            .ok_or_else(|| ParseSettingError::new("`slow`, `normal` or `fast`", s))
    }
}

impl Camera {
    /// The Smart Composition framing style.
    pub fn framing(&self) -> Result<Framing> {
        let payload = self.xu_read(control::FRAMING)?;
        Framing::ALL
            .into_iter()
            .find(|framing| payload.first() == Some(&framing.code()))
            .ok_or_else(|| Error::Protocol(format!("unrecognized framing bytes {payload:02x?}")))
    }

    /// Sets the Smart Composition framing style, and waits until the camera has
    /// taken the new value.
    ///
    /// The camera only reframes when AI Zoom is enabled
    /// ([`Features::ai_zoom`](crate::Features::ai_zoom)). Otherwise, the style
    /// is stored but has no effect on the image.
    ///
    /// Choosing [`Framing::Head`], [`Framing::HalfBody`] or
    /// [`Framing::WholeBody`] hands the zoom to the camera, replacing any zoom
    /// set by hand. Zooming by hand afterward makes the camera switch the
    /// style to [`Framing::Manual`].
    ///
    /// # Errors
    ///
    /// Fails with [`Error::NotStreaming`] unless video is streaming.
    pub fn set_framing(&self, framing: Framing) -> Result<()> {
        if self.framing().ok() == Some(framing) {
            return Ok(());
        }
        self.require_streaming()?;
        self.xu_write(control::FRAMING, &[framing.code()])?;
        self.settle(|| Ok(self.framing()? == framing))
    }

    /// How fast tracking follows its subject.
    pub fn tracking_speed(&self) -> Result<TrackingSpeed> {
        let payload = self.xu_read(control::TRACKING_SPEED)?;
        TrackingSpeed::ALL
            .into_iter()
            .find(|speed| payload.first() == Some(&speed.code()))
            .ok_or_else(|| {
                Error::Protocol(format!("unrecognized tracking speed bytes {payload:02x?}"))
            })
    }

    /// Sets how fast tracking follows its subject, and waits until the camera
    /// has taken the new value.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::NotStreaming`] unless video is streaming.
    pub fn set_tracking_speed(&self, speed: TrackingSpeed) -> Result<()> {
        if self.tracking_speed().ok() == Some(speed) {
            return Ok(());
        }
        self.require_streaming()?;
        self.write_verified(control::TRACKING_SPEED, &[speed.code()], || {
            Ok(self.tracking_speed()? == speed)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeCamera;

    const STREAMING: [u8; 5] = [0x2b, 0x28, 0x00, 0x01, 0x00];
    const IDLE: [u8; 5] = [0x2b, 0x28, 0x00, 0x00, 0x00];

    fn framing_camera() -> FakeCamera {
        FakeCamera::new()
            .selector(control::DEVICE_STATUS, &STREAMING)
            .selector(control::FRAMING, &[1])
    }

    #[test]
    fn framing_round_trips() {
        let fake = framing_camera();
        let camera = fake.camera();
        assert_eq!(camera.framing().unwrap(), Framing::Head);
        camera.set_framing(Framing::WholeBody).unwrap();
        assert_eq!(fake.xu_writes(), [(9, control::FRAMING.id, vec![3])]);
        assert_eq!(camera.framing().unwrap(), Framing::WholeBody);
        // Already there: no write, and no need to be streaming.
        fake.poke(control::DEVICE_STATUS, &IDLE);
        camera.set_framing(Framing::WholeBody).unwrap();
        assert_eq!(fake.xu_writes().len(), 1);
    }

    #[test]
    fn framing_reads_the_hand_set_zoom_state() {
        let fake = framing_camera();
        let camera = fake.camera();
        // What the camera reports after the zoom is changed by hand.
        fake.poke(control::FRAMING, &[4]);
        assert_eq!(camera.framing().unwrap(), Framing::Manual);
        fake.poke(control::FRAMING, &[9]);
        assert!(matches!(camera.framing(), Err(Error::Protocol(_))));
    }

    #[test]
    fn tracking_speed_round_trips() {
        let fake = framing_camera().selector(control::TRACKING_SPEED, &[1]);
        let camera = fake.camera();
        assert_eq!(camera.tracking_speed().unwrap(), TrackingSpeed::Normal);
        camera.set_tracking_speed(TrackingSpeed::Fast).unwrap();
        assert_eq!(fake.xu_writes(), [(9, control::TRACKING_SPEED.id, vec![0])]);
        assert_eq!(camera.tracking_speed().unwrap(), TrackingSpeed::Fast);
        camera.set_tracking_speed(TrackingSpeed::Fast).unwrap();
        assert_eq!(fake.xu_writes().len(), 1, "already there: no write");

        fake.poke(control::TRACKING_SPEED, &[7]);
        assert!(matches!(camera.tracking_speed(), Err(Error::Protocol(_))));
        assert!(matches!(
            framing_camera().camera().tracking_speed(),
            Err(Error::Unsupported)
        ));
    }

    #[test]
    fn refuses_to_write_while_not_streaming() {
        let fake = framing_camera();
        fake.poke(control::DEVICE_STATUS, &IDLE);
        assert!(matches!(
            fake.camera().set_framing(Framing::WholeBody),
            Err(Error::NotStreaming)
        ));
        assert!(fake.xu_writes().is_empty());
    }

    #[test]
    fn reports_a_change_the_camera_ignores() {
        let fake = framing_camera().swallow_xu_writes();
        assert!(matches!(
            fake.camera().set_framing(Framing::HalfBody),
            Err(Error::NotApplied)
        ));
    }

    #[test]
    fn cameras_without_the_selectors_report_unsupported() {
        let camera = FakeCamera::new().camera();
        assert!(matches!(camera.framing(), Err(Error::Unsupported)));
        assert!(matches!(camera.tracking_speed(), Err(Error::Unsupported)));
    }

    #[test]
    fn text_forms_round_trip() {
        for framing in Framing::ALL {
            assert_eq!(framing.to_string().parse::<Framing>().unwrap(), framing);
        }
        for speed in TrackingSpeed::ALL {
            assert_eq!(speed.to_string().parse::<TrackingSpeed>().unwrap(), speed);
        }
        assert!("full".parse::<Framing>().is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn config_words_match_the_text_forms_of_framing() {
        for value in Framing::ALL {
            let word = serde_json::to_value(value).unwrap();
            assert_eq!(word, value.to_string());
            assert_eq!(serde_json::from_value::<Framing>(word).unwrap(), value);
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn config_words_match_the_text_forms_of_trackingspeed() {
        for value in TrackingSpeed::ALL {
            let word = serde_json::to_value(value).unwrap();
            assert_eq!(word, value.to_string());
            assert_eq!(
                serde_json::from_value::<TrackingSpeed>(word).unwrap(),
                value
            );
        }
    }
}
