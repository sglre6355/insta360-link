//! How the microphone treats what it hears.

use std::fmt;
use std::str::FromStr;

use crate::camera::Camera;
use crate::device::selectors::aux;
use crate::error::ParseSettingError;
use crate::error::{Error, Result};

/// The microphone's processing mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "kebab-case")
)]
pub enum AudioMode {
    /// Picks out your voice and filters out other voices and noise.
    VoiceSuppression,
    /// Reduces background noise and makes your voice clearer. The default.
    VoiceFocus,
    /// Balances your voice with music.
    MusicBalance,
}

impl AudioMode {
    const ALL: [Self; 3] = [Self::VoiceSuppression, Self::VoiceFocus, Self::MusicBalance];

    const fn code(self) -> u8 {
        match self {
            Self::VoiceSuppression => 0,
            Self::VoiceFocus => 1,
            Self::MusicBalance => 2,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::VoiceSuppression => "voice-suppression",
            Self::VoiceFocus => "voice-focus",
            Self::MusicBalance => "music-balance",
        }
    }
}

impl fmt::Display for AudioMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for AudioMode {
    type Err = ParseSettingError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.name() == s)
            .ok_or_else(|| {
                ParseSettingError::new("`voice-suppression`, `voice-focus` or `music-balance`", s)
            })
    }
}

impl Camera {
    /// The microphone's processing mode.
    pub fn audio_mode(&self) -> Result<AudioMode> {
        let payload = self.xu_read(aux::AUDIO_MODE)?;
        AudioMode::ALL
            .into_iter()
            .find(|mode| payload.first() == Some(&mode.code()))
            .ok_or_else(|| Error::Protocol(format!("unrecognized audio mode bytes {payload:02x?}")))
    }

    /// Sets the microphone's processing mode, and waits until the camera has
    /// taken the new value.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::NotStreaming`] unless video is streaming.
    pub fn set_audio_mode(&self, mode: AudioMode) -> Result<()> {
        if self.audio_mode().ok() == Some(mode) {
            return Ok(());
        }
        self.require_streaming()?;
        self.write_verified(aux::AUDIO_MODE, &[mode.code()], || {
            Ok(self.audio_mode()? == mode)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeCamera;

    #[test]
    fn audio_mode_round_trips() {
        let fake = FakeCamera::new().selector(aux::AUDIO_MODE, &[1]);
        let camera = fake.camera();
        assert_eq!(camera.audio_mode().unwrap(), AudioMode::VoiceFocus);
        camera.set_audio_mode(AudioMode::MusicBalance).unwrap();
        assert_eq!(fake.xu_writes(), [(10, aux::AUDIO_MODE.id, vec![2])]);
        assert_eq!(camera.audio_mode().unwrap(), AudioMode::MusicBalance);
        camera.set_audio_mode(AudioMode::MusicBalance).unwrap();
        assert_eq!(fake.xu_writes().len(), 1, "already there: no write");

        fake.poke(aux::AUDIO_MODE, &[9]);
        assert!(matches!(camera.audio_mode(), Err(Error::Protocol(_))));
        assert!(matches!(
            FakeCamera::new().camera().audio_mode(),
            Err(Error::Unsupported)
        ));
    }

    #[test]
    fn text_forms_round_trip() {
        for mode in AudioMode::ALL {
            assert_eq!(mode.to_string().parse::<AudioMode>().unwrap(), mode);
        }
        assert!("music".parse::<AudioMode>().is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn config_words_match_the_text_forms_of_audiomode() {
        for value in AudioMode::ALL {
            let word = serde_json::to_value(value).unwrap();
            assert_eq!(word, value.to_string());
            assert_eq!(serde_json::from_value::<AudioMode>(word).unwrap(), value);
        }
    }
}
