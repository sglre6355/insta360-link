//! Camera modes: standard webcam, subject tracking, whiteboard, overhead,
//! DeskView, and Auto Framing.
//!
//! Modes are controlled through vendor extension units. The camera only applies
//! mode changes while video is streaming, and applying them takes some time.
//! Each write is therefore followed by repeated reads until the requested value
//! takes effect.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use crate::camera::Camera;
use crate::device::selectors::{ai, control};
use crate::error::{Error, ParseSettingError, Result};

/// What the camera reports in place of a mode while it is between modes.
const BUSY: u8 = 0xff;

/// How long a mode change keeps the camera busy at most. It has been
/// measured at between one and three seconds.
#[cfg(not(test))]
const BUSY_GRACE: Duration = Duration::from_secs(3);
#[cfg(test)]
const BUSY_GRACE: Duration = Duration::from_millis(10);

/// Camera operating mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "kebab-case")
)]
#[non_exhaustive]
pub enum CameraMode {
    /// Behaves as a plain webcam.
    Normal,
    /// Follows a person.
    #[cfg_attr(feature = "serde", serde(rename = "track"))]
    Tracking,
    /// Finds a whiteboard and straightens it.
    Whiteboard,
    /// Points down for a top-down view of the desk.
    Overhead,
    /// Shows the desk in front of the camera, corrected for perspective.
    #[cfg_attr(feature = "serde", serde(rename = "deskview"))]
    DeskView,
    /// Keeps everyone in view, reframing as people come and go. The camera
    /// recenters when this engages.
    AutoFraming,
}

impl CameraMode {
    /// Every mode, in a stable order.
    pub const ALL: [Self; 6] = [
        Self::Normal,
        Self::Tracking,
        Self::Whiteboard,
        Self::Overhead,
        Self::DeskView,
        Self::AutoFraming,
    ];

    /// The byte that identifies this mode in the view state and in a preset.
    pub(crate) const fn id(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::Tracking => 1,
            Self::Whiteboard => 4,
            Self::Overhead => 5,
            Self::DeskView => 6,
            Self::AutoFraming => 7,
        }
    }

    /// The mode an id stands for.
    pub(crate) fn from_id(id: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.id() == id)
    }

    fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Tracking => "track",
            Self::Whiteboard => "whiteboard",
            Self::Overhead => "overhead",
            Self::DeskView => "deskview",
            Self::AutoFraming => "auto-framing",
        }
    }
}

impl fmt::Display for CameraMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for CameraMode {
    type Err = ParseSettingError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.name() == s)
            .ok_or_else(|| {
                ParseSettingError::new(
                    "`normal`, `track`, `whiteboard`, `overhead`, `deskview` or `auto-framing`",
                    s,
                )
            })
    }
}

/// The camera's reported mode status.
///
/// Status information may persist after the camera leaves the corresponding
/// mode, so interpret it alongside [`Camera::mode`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ModeStatus {
    /// Whiteboard mode is looking for a whiteboard.
    DetectingWhiteboard,
    /// Whiteboard mode found a whiteboard and is showing it.
    WhiteboardFound,
    /// Whiteboard mode found no whiteboard.
    WhiteboardNotFound,
    /// DeskView could not be entered.
    DeskViewFailed,
    /// A status this crate has no name for. `0x10` has been seen while DeskView
    /// is running and for some time after it is left.
    Other(u8),
}

impl ModeStatus {
    /// Interprets the status byte of the view state. Zero means there is
    /// nothing to report.
    fn from_view_state(mode_id: u8, status: u8) -> Option<Self> {
        match status {
            0 => None,
            1 => Some(Self::DetectingWhiteboard),
            2 => Some(Self::WhiteboardFound),
            3 => Some(Self::WhiteboardNotFound),
            0x11 if mode_id == CameraMode::DeskView.id() => Some(Self::DeskViewFailed),
            other => Some(Self::Other(other)),
        }
    }
}

impl Camera {
    /// Reads the mode and status bytes, or fails with [`Error::Busy`] while the
    /// camera is between modes. A reading taken then says nothing about either.
    fn mode_and_status(&self) -> Result<(CameraMode, Option<ModeStatus>)> {
        let state = self.view_state()?;
        let id = state.mode_id()?;
        if id == BUSY {
            return Err(Error::Busy);
        }
        let mode = CameraMode::from_id(id)
            .ok_or_else(|| Error::Protocol(format!("unrecognized mode {id:#04x}")))?;
        Ok((mode, ModeStatus::from_view_state(id, state.status()?)))
    }

    /// The camera's current mode.
    ///
    /// A camera that is not streaming reports [`CameraMode::Normal`] whatever
    /// it will do once video starts.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::Busy`] while the camera is between modes. That lasts
    /// a second or so during a mode change. It also lasts for as long as
    /// tracking has been asked for and no one is in view, because the camera
    /// only reports [`CameraMode::Tracking`] once it has someone to follow.
    pub fn mode(&self) -> Result<CameraMode> {
        self.mode_and_status().map(|(mode, _)| mode)
    }

    /// What the camera reports about the mode it is in, or `None` if it reports
    /// nothing.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::Busy`] while the camera is between modes.
    pub fn mode_status(&self) -> Result<Option<ModeStatus>> {
        self.mode_and_status().map(|(_, status)| status)
    }

    /// The mode once the camera has stopped being busy, or `None` if it stays
    /// busy.
    fn mode_once_settled(&self) -> Result<Option<CameraMode>> {
        self.poll_within(BUSY_GRACE, || match self.mode() {
            Ok(mode) => Ok(Some(mode)),
            Err(Error::Busy) => Ok(None),
            Err(err) => Err(err),
        })
    }

    /// Sets the camera mode and waits until it has taken effect.
    ///
    /// Switching between special modes requires passing through
    /// [`CameraMode::Normal`], as the camera cannot switch between them
    /// directly.
    ///
    /// Switching to [`CameraMode::Tracking`] succeeds once the camera accepts
    /// the request. If no person is in view, the camera remains busy and
    /// [`Camera::mode`] returns [`Error::Busy`] until someone appears.
    ///
    /// The camera manages the gimbal position during mode changes.
    /// [`CameraMode::DeskView`] tilts to its previously used angle, while
    /// [`CameraMode::Overhead`] points straight down. Both restore the previous
    /// position when exited. [`CameraMode::Whiteboard`] leaves the gimbal in
    /// place. Adjusting the gimbal in DeskView also updates the angle used next
    /// time.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::NotStreaming`] unless video is streaming.
    pub fn set_mode(&self, mode: CameraMode) -> Result<()> {
        self.require_streaming()?;
        let current = match self.mode_once_settled()? {
            Some(current) => current,
            // Still busy after any mode change would have finished, so tracking
            // is waiting for someone to come into view.
            None if mode == CameraMode::Tracking => return Ok(()),
            None => {
                self.switch_mode(CameraMode::Tracking, CameraMode::Normal)?;
                CameraMode::Normal
            }
        };
        if current == mode {
            return Ok(());
        }
        let from = if current != CameraMode::Normal && mode != CameraMode::Normal {
            self.switch_mode(current, CameraMode::Normal)?;
            CameraMode::Normal
        } else {
            current
        };
        self.switch_mode(from, mode)
    }

    /// Performs one mode change.
    fn switch_mode(&self, from: CameraMode, to: CameraMode) -> Result<()> {
        // Busy counts as arrived only for tracking, which stays busy until it
        // has someone to follow.
        let arrived = || match self.mode() {
            Ok(mode) => Ok(mode == to),
            Err(Error::Busy) => Ok(to == CameraMode::Tracking),
            Err(err) => Err(err),
        };

        // Tracking has a dedicated switch. Cameras without it fall through to
        // the view state.
        let tracking_switch = match (from, to) {
            (_, CameraMode::Tracking) => Some(true),
            (CameraMode::Tracking, CameraMode::Normal) => Some(false),
            _ => None,
        };
        if let Some(on) = tracking_switch {
            match self.xu_write(ai::TRACKING, &[u8::from(on)]) {
                Ok(()) => return self.settle(arrived),
                Err(Error::Unsupported) => {}
                Err(err) => return Err(err),
            }
        }

        let request = self
            .view_state()?
            .mode_request(to.id(), self.info().model)?;
        self.xu_write(control::VIEW_STATE, &request)?;
        self.settle(arrived)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::transport::Control;
    use crate::testing::FakeCamera;

    const STREAMING: [u8; 5] = [0x2b, 0x28, 0x00, 0x01, 0x00];
    const IDLE: [u8; 5] = [0x2b, 0x28, 0x00, 0x00, 0x00];

    /// A full-length view state of 61 bytes, with markers in the tail that a
    /// mode request must not copy.
    fn view_state(code: [u8; 2]) -> Vec<u8> {
        let mut payload = vec![0u8; 61];
        payload[..2].copy_from_slice(&code);
        payload[40] = 0x77;
        payload[60] = 0x01;
        payload
    }

    fn mode_camera() -> FakeCamera {
        FakeCamera::new()
            .selector(control::VIEW_STATE, &view_state([0, 0]))
            .selector(control::DEVICE_STATUS, &STREAMING)
            .selector(ai::TRACKING, &[0])
    }

    /// Like a real camera, reflects the tracking switch in the mode byte.
    fn mirror_tracking(fake: &FakeCamera) {
        let on = fake
            .xu_writes()
            .last()
            .is_some_and(|w| w.0 == 11 && w.2 == [1]);
        fake.poke(control::VIEW_STATE, &view_state([u8::from(on), 0]));
    }

    #[test]
    fn reads_modes() {
        let fake = mode_camera();
        let camera = fake.camera();
        for mode in CameraMode::ALL {
            fake.poke(control::VIEW_STATE, &view_state([mode.id(), 0]));
            assert_eq!(camera.mode().unwrap(), mode);
            assert_eq!(camera.mode_status().unwrap(), None);
        }
        fake.poke(control::VIEW_STATE, &view_state([0x09, 0x00]));
        assert!(matches!(camera.mode(), Err(Error::Protocol(_))));
    }

    #[test]
    fn a_busy_reading_says_nothing() {
        // Captured in the middle of mode changes and while tracking had no one
        // to follow.
        let fake = mode_camera();
        let camera = fake.camera();
        for status in [0x00, 0x01, 0x02, 0x10, 0x11] {
            fake.poke(control::VIEW_STATE, &view_state([0xff, status]));
            assert!(matches!(camera.mode(), Err(Error::Busy)));
            assert!(matches!(camera.mode_status(), Err(Error::Busy)));
        }
    }

    #[test]
    fn reads_the_status_as_link_controller_does() {
        let fake = mode_camera();
        let camera = fake.camera();
        let cases = [
            ([0x04, 0x01], Some(ModeStatus::DetectingWhiteboard)),
            ([0x04, 0x02], Some(ModeStatus::WhiteboardFound)),
            ([0x04, 0x03], Some(ModeStatus::WhiteboardNotFound)),
            ([0x06, 0x11], Some(ModeStatus::DeskViewFailed)),
            // Reported while DeskView is running, and after it has been left.
            ([0x06, 0x10], Some(ModeStatus::Other(0x10))),
            ([0x00, 0x10], Some(ModeStatus::Other(0x10))),
            // The failure code only means that in DeskView.
            ([0x00, 0x11], Some(ModeStatus::Other(0x11))),
            ([0x05, 0x00], None),
        ];
        for (bytes, status) in cases {
            fake.poke(control::VIEW_STATE, &view_state(bytes));
            assert_eq!(camera.mode_status().unwrap(), status, "{bytes:02x?}");
        }
    }

    #[test]
    fn a_mode_request_sets_only_the_mode() {
        let fake = mode_camera();
        fake.camera().set_mode(CameraMode::DeskView).unwrap();
        let writes = fake.xu_writes();
        assert_eq!(writes.len(), 1);
        let (unit, selector, payload) = &writes[0];
        assert_eq!((*unit, *selector), (9, control::VIEW_STATE.id));
        assert_eq!(payload.len(), 61);
        // The mode, with the status byte left for the camera to fill in.
        assert_eq!(payload[..2], [0x06, 0x00]);
        assert!(payload[2..38].iter().all(|&byte| byte == 0));
        // Pan, tilt and a third angle marked as not set.
        let word = |at: usize| i32::from_le_bytes(payload[at..at + 4].try_into().unwrap());
        assert_eq!([word(38), word(42), word(46)], [3610, 3610, 3610]);
    }

    #[test]
    fn mode_changes_never_move_the_gimbal_themselves() {
        let fake = mode_camera()
            .control(Control::Pan, -522_000, 522_000, 3600, 0)
            .control(Control::Tilt, -324_000, 360_000, 3600, 0);
        let camera = fake.camera();
        for mode in [
            CameraMode::DeskView,
            CameraMode::Normal,
            CameraMode::Whiteboard,
            CameraMode::Overhead,
        ] {
            camera.set_mode(mode).unwrap();
        }
        assert!(fake.writes().is_empty());
    }

    #[test]
    fn auto_framing_is_mode_seven() {
        let fake = mode_camera();
        let camera = fake.camera();
        camera.set_mode(CameraMode::AutoFraming).unwrap();
        assert_eq!(fake.xu_writes()[0].2[..2], [0x07, 0x00]);
        assert_eq!(camera.mode().unwrap(), CameraMode::AutoFraming);
    }

    #[test]
    fn tracking_with_no_one_in_view_leaves_the_camera_busy() {
        // Captured after the tracking switch was written with nobody in frame:
        // the mode stays busy and the switch reads off.
        let fake = mode_camera();
        fake.script(control::VIEW_STATE, [view_state([0x00, 0x10])]);
        fake.poke(control::VIEW_STATE, &view_state([0xff, 0x10]));
        let camera = fake.camera();
        camera.set_mode(CameraMode::Tracking).unwrap();
        assert_eq!(fake.xu_writes(), [(11, ai::TRACKING.id, vec![1])]);
        assert!(matches!(camera.mode(), Err(Error::Busy)));

        // Asking again while it waits changes nothing.
        camera.set_mode(CameraMode::Tracking).unwrap();
        assert_eq!(fake.xu_writes().len(), 1);
    }

    #[test]
    fn a_camera_that_stays_busy_is_taken_out_of_tracking_first() {
        let fake = mode_camera();
        fake.poke(control::VIEW_STATE, &view_state([0xff, 0x00]));
        // The fake does not link the switch to the mode byte, so the wait for
        // normal mode times out. The write is what matters here.
        let err = fake.camera().set_mode(CameraMode::Overhead).unwrap_err();
        assert!(matches!(err, Error::NotApplied));
        assert_eq!(fake.xu_writes(), [(11, ai::TRACKING.id, vec![0])]);
    }

    #[test]
    fn setting_the_current_mode_writes_nothing() {
        let fake = mode_camera();
        fake.camera().set_mode(CameraMode::Normal).unwrap();
        assert!(fake.xu_writes().is_empty());
    }

    #[test]
    fn special_modes_are_left_through_normal() {
        let fake = mode_camera();
        fake.poke(control::VIEW_STATE, &view_state([0x06, 0x10]));
        fake.camera().set_mode(CameraMode::Whiteboard).unwrap();
        let codes: Vec<_> = fake.xu_writes().iter().map(|w| w.2[..2].to_vec()).collect();
        assert_eq!(codes, [[0x00, 0x00], [0x04, 0x00]]);
    }

    #[test]
    fn tracking_uses_its_own_switch() {
        let fake = mode_camera();
        // The fake does not link the switch to the mode byte, so the wait for
        // the mode to change times out. The write is what matters here.
        let err = fake.camera().set_mode(CameraMode::Tracking).unwrap_err();
        assert!(matches!(err, Error::NotApplied));
        assert_eq!(fake.xu_writes(), [(11, ai::TRACKING.id, vec![1])]);

        mirror_tracking(&fake);
        assert_eq!(fake.camera().mode().unwrap(), CameraMode::Tracking);
        let _ = fake.camera().set_mode(CameraMode::Normal);
        assert_eq!(
            fake.xu_writes().last().unwrap(),
            &(11, ai::TRACKING.id, vec![0])
        );
    }

    #[test]
    fn tracking_falls_back_to_the_view_state_without_the_switch() {
        let fake = FakeCamera::new()
            .selector(control::VIEW_STATE, &[0u8; 52])
            .selector(control::DEVICE_STATUS, &STREAMING);
        fake.camera().set_mode(CameraMode::Tracking).unwrap();
        let writes = fake.xu_writes();
        assert_eq!(writes[0].2.len(), 52);
        assert_eq!(writes[0].2[..2], [0x01, 0x00]);
        assert_eq!(fake.camera().mode().unwrap(), CameraMode::Tracking);
    }

    #[test]
    fn refuses_to_write_while_not_streaming() {
        let fake = mode_camera();
        fake.poke(control::DEVICE_STATUS, &IDLE);
        let camera = fake.camera();
        assert!(!camera.is_streaming().unwrap());
        assert!(matches!(
            camera.set_mode(CameraMode::DeskView),
            Err(Error::NotStreaming)
        ));
        assert!(fake.xu_writes().is_empty());
    }

    #[test]
    fn reports_a_change_the_camera_ignores() {
        let fake = mode_camera().swallow_xu_writes();
        assert!(matches!(
            fake.camera().set_mode(CameraMode::Overhead),
            Err(Error::NotApplied)
        ));
    }

    #[test]
    fn cameras_without_the_selectors_report_unsupported() {
        let camera = FakeCamera::new().camera();
        assert!(matches!(camera.mode(), Err(Error::Unsupported)));
        assert!(matches!(
            camera.set_mode(CameraMode::Tracking),
            Err(Error::Unsupported)
        ));
    }

    #[test]
    fn text_forms_round_trip() {
        for mode in CameraMode::ALL {
            assert_eq!(mode.to_string().parse::<CameraMode>().unwrap(), mode);
        }
        assert!("tracking".parse::<CameraMode>().is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn config_words_match_the_text_forms_of_cameramode() {
        for value in CameraMode::ALL {
            let word = serde_json::to_value(value).unwrap();
            assert_eq!(word, value.to_string());
            assert_eq!(serde_json::from_value::<CameraMode>(word).unwrap(), value);
        }
    }
}
