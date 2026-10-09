//! Privacy mode: the gimbal parked face-down with a placeholder picture in
//! place of the video.

use crate::camera::Camera;
use crate::device::selectors::aux;
use crate::error::{Error, Result};

/// Payloads written to the privacy switch. The first byte turns privacy mode on
/// or off and the second is unused.
///
/// The switch is write-only in effect. Reading its selector returns `02 00`
/// whether privacy mode is on or off, so the current state is read from the
/// view state instead.
const PRIVACY_ON: [u8; 2] = [0x01, 0x00];
const PRIVACY_OFF: [u8; 2] = [0x00, 0x00];

/// Product ids of the cameras whose view state does not report privacy mode.
/// `4c01` is one of the two ids of the original Link.
const PRODUCTS_WITHOUT_PRIVACY_FLAG: [u16; 1] = [0x4c01];

impl Camera {
    /// Returns whether the camera is in privacy mode.
    ///
    /// A downward-facing gimbal alone does not indicate privacy mode. It also
    /// points down in overhead mode, after being tilted there by command, and
    /// while no video is streaming, and this returns `false` in all three
    /// cases.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::Unsupported`] on a camera that does not report
    /// the mode.
    pub fn privacy(&self) -> Result<bool> {
        if PRODUCTS_WITHOUT_PRIVACY_FLAG.contains(&self.info().product_id) {
            return Err(Error::Unsupported);
        }
        self.view_state()?.privacy().ok_or(Error::Unsupported)
    }

    /// Enters or leaves privacy mode, and waits until the camera reports the
    /// change.
    ///
    /// Entering parks the gimbal face-down and replaces the video with a
    /// placeholder. The camera ignores gimbal moves until privacy mode is left
    /// again. Leaving returns the gimbal to where it was.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::NotStreaming`] unless video is streaming.
    ///
    /// Fails with [`Error::Unsupported`] on a camera that does not report the
    /// mode, because the change could not be checked.
    pub fn set_privacy(&self, on: bool) -> Result<()> {
        self.require_streaming()?;
        if self.privacy()? == on {
            return Ok(());
        }
        let payload = if on { PRIVACY_ON } else { PRIVACY_OFF };
        self.write_verified(aux::PRIVACY, &payload, || Ok(self.privacy()? == on))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::selectors::control;
    use crate::testing::FakeCamera;

    const STREAMING: [u8; 5] = [0x2b, 0x28, 0x00, 0x01, 0x00];
    const IDLE: [u8; 5] = [0x2b, 0x28, 0x00, 0x00, 0x00];

    /// A view state with the privacy flag as given.
    fn view_state(privacy: bool) -> Vec<u8> {
        let mut payload = vec![0u8; 61];
        payload[54] = 0x22 | u8::from(privacy);
        payload
    }

    /// A streaming camera with the privacy switch and the flag as given.
    fn privacy_camera(privacy: bool) -> FakeCamera {
        FakeCamera::new()
            .selector(control::DEVICE_STATUS, &STREAMING)
            .selector(control::VIEW_STATE, &view_state(privacy))
            // The switch reads back the same whatever state the camera is in.
            .selector(aux::PRIVACY, &[0x02, 0x00])
    }

    #[test]
    fn privacy_is_read_from_the_flag() {
        assert!(privacy_camera(true).camera().privacy().unwrap());
        assert!(!privacy_camera(false).camera().privacy().unwrap());
    }

    #[test]
    fn privacy_switch_is_written_and_checked_against_the_flag() {
        // Entering: the flag reads clear once, then set.
        let fake = privacy_camera(true);
        fake.script(control::VIEW_STATE, [view_state(false)]);
        let camera = fake.camera();
        camera.set_privacy(true).unwrap();
        assert_eq!(
            fake.xu_writes(),
            [(10, aux::PRIVACY.id, PRIVACY_ON.to_vec())]
        );

        // Already in privacy mode: nothing to write.
        camera.set_privacy(true).unwrap();
        assert_eq!(fake.xu_writes().len(), 1);

        // Leaving: the fake never clears the flag, so the wait gives up, but
        // the right value was sent exactly once.
        assert!(matches!(camera.set_privacy(false), Err(Error::NotApplied)));
        assert_eq!(
            fake.xu_writes().last().unwrap(),
            &(10, aux::PRIVACY.id, PRIVACY_OFF.to_vec())
        );
        assert_eq!(fake.xu_writes().len(), 2);
    }

    #[test]
    fn cameras_that_do_not_report_the_mode_are_unsupported() {
        // A view state that ends before the flag.
        let short = FakeCamera::new()
            .selector(control::DEVICE_STATUS, &STREAMING)
            .selector(control::VIEW_STATE, &[0u8; 52])
            .selector(aux::PRIVACY, &PRIVACY_ON);
        let none = FakeCamera::new()
            .selector(control::DEVICE_STATUS, &STREAMING)
            .selector(aux::PRIVACY, &PRIVACY_ON);
        for fake in [short, none] {
            let camera = fake.camera();
            assert!(matches!(camera.privacy(), Err(Error::Unsupported)));
            assert!(matches!(camera.set_privacy(true), Err(Error::Unsupported)));
            // Nothing is written that could not be checked.
            assert!(fake.xu_writes().is_empty());
        }
    }

    #[test]
    fn the_flag_is_not_read_on_the_model_link_controller_skips() {
        // Even with a view state long enough to carry the flag.
        let fake = privacy_camera(true);
        let camera = fake.camera_with_product_id(0x4c01);
        assert!(matches!(camera.privacy(), Err(Error::Unsupported)));
        assert!(matches!(camera.set_privacy(false), Err(Error::Unsupported)));
        assert!(fake.xu_writes().is_empty());
        // The other id of the original Link is read like the rest.
        assert!(fake.camera_with_product_id(0x4c02).privacy().unwrap());
    }

    #[test]
    fn privacy_needs_a_stream_and_the_switch() {
        let idle = privacy_camera(false);
        idle.poke(control::DEVICE_STATUS, &IDLE);
        assert!(matches!(
            idle.camera().set_privacy(true),
            Err(Error::NotStreaming)
        ));
        let no_switch = FakeCamera::new()
            .selector(control::DEVICE_STATUS, &STREAMING)
            .selector(control::VIEW_STATE, &view_state(false));
        assert!(matches!(
            no_switch.camera().set_privacy(true),
            Err(Error::Unsupported)
        ));
    }
}
