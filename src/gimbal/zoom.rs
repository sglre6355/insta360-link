//! Zoom control.
//!
//! Zoom is set through the standard UVC control, which only reports the last
//! commanded value. On cameras that support it, the actual zoom level is
//! therefore read from the view state.

use std::ops::RangeInclusive;

use super::to_raw;
use crate::camera::Camera;
use crate::device::transport::Control;
use crate::error::{Optional, Result};

/// The zoom control counts in hundredths.
const UNITS_PER_ZOOM: f64 = 100.0;

impl Camera {
    /// The zoom factors the camera accepts, such as `1.0..=4.0`.
    pub fn zoom_limits(&self) -> Result<RangeInclusive<f64>> {
        let info = self.control_info(Control::Zoom)?;
        Ok(f64::from(info.minimum) / UNITS_PER_ZOOM..=f64::from(info.maximum) / UNITS_PER_ZOOM)
    }

    /// The current zoom factor, where `1.0` is the full field of view.
    ///
    /// This is the zoom actually in effect, including changes made by gestures,
    /// Smart Composition or a mode such as DeskView, on cameras that report it.
    /// Otherwise it is the last zoom that was set.
    pub fn zoom(&self) -> Result<f64> {
        let info = self.control_info(Control::Zoom)?;
        let reported = self
            .view_state()
            .optional()?
            .and_then(|state| state.zoom())
            .map(i32::from);
        let raw = match reported {
            Some(zoom) if (info.minimum..=info.maximum).contains(&zoom) => zoom,
            _ => self.get_control(Control::Zoom)?,
        };
        Ok(f64::from(raw) / UNITS_PER_ZOOM)
    }

    /// Converts a zoom factor to the value of the zoom control, or fails if the
    /// camera does not accept it.
    pub(crate) fn zoom_raw(&self, factor: f64) -> Result<i32> {
        let info = self.control_info(Control::Zoom)?;
        to_raw("zoom", factor, UNITS_PER_ZOOM, &info)
    }

    /// Sets the zoom factor.
    pub fn set_zoom(&self, factor: f64) -> Result<()> {
        let raw = self.zoom_raw(factor)?;
        self.set_control(Control::Zoom, raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::selectors::control;
    use crate::error::Error;
    use crate::testing::FakeCamera;

    fn zoom_camera() -> FakeCamera {
        FakeCamera::new().control(Control::Zoom, 100, 400, 1, 100)
    }

    /// A view state long enough to carry the zoom, reporting `zoom` hundredths.
    fn view_state(zoom: u16) -> Vec<u8> {
        let mut payload = vec![0u8; 61];
        payload[50..52].copy_from_slice(&zoom.to_le_bytes());
        payload
    }

    #[test]
    fn zoom_in_effect_wins_over_the_last_commanded_one() {
        // After a gesture zoom the standard control still says 1.0.
        let fake = zoom_camera().selector(control::VIEW_STATE, &view_state(180));
        assert_eq!(fake.get(Control::Zoom), 100);
        assert_eq!(fake.camera().zoom().unwrap(), 1.8);
    }

    #[test]
    fn zoom_falls_back_when_the_payload_cannot_tell() {
        // A payload that ends before the zoom field.
        let short = zoom_camera().selector(control::VIEW_STATE, &[0u8; 50]);
        // An implausible value is ignored as well.
        let wild = zoom_camera().selector(control::VIEW_STATE, &view_state(0));
        for fake in [short, wild] {
            fake.set(Control::Zoom, 250);
            assert_eq!(fake.camera().zoom().unwrap(), 2.5);
        }
    }
    #[test]
    fn zoom_is_a_factor() {
        let fake = zoom_camera();
        let camera = fake.camera();
        assert_eq!(camera.zoom_limits().unwrap(), 1.0..=4.0);
        camera.set_zoom(2.5).unwrap();
        assert_eq!(fake.get(Control::Zoom), 250);
        assert_eq!(camera.zoom().unwrap(), 2.5);
        assert!(matches!(
            camera.set_zoom(4.5),
            Err(Error::OutOfRange { what: "zoom", .. })
        ));
        assert!(matches!(
            camera.set_zoom(f64::NAN),
            Err(Error::OutOfRange { .. })
        ));
    }
}
