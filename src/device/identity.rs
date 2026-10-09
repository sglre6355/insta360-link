//! Firmware version and serial number.

use crate::camera::Camera;
use crate::device::selectors::control;
use crate::error::{Error, Result};

/// Byte range of the 64-byte firmware version string in the device-information
/// payload, following fields of 33 and 64 bytes.
const FIRMWARE_FIELD: std::ops::Range<usize> = 97..161;

/// Reads a NUL-padded printable ASCII string, or `None` if the field does not
/// hold one.
fn c_string(field: &[u8]) -> Option<String> {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    let text = &field[..end];
    let readable = !text.is_empty() && text.iter().all(|b| b.is_ascii_graphic() || *b == b' ');
    readable.then(|| String::from_utf8_lossy(text).into_owned())
}

impl Camera {
    /// The firmware version the camera reports, such as `v3.2.7.8_build2`.
    pub fn firmware_version(&self) -> Result<String> {
        let data = self.xu_read(control::DEVICE_INFO)?;
        data.get(FIRMWARE_FIELD).and_then(c_string).ok_or_else(|| {
            Error::Protocol("the device information holds no firmware version".into())
        })
    }

    /// The serial number printed on the camera.
    pub fn serial_number(&self) -> Result<String> {
        c_string(&self.xu_read(control::SERIAL_NUMBER)?)
            .ok_or_else(|| Error::Protocol("the serial number is not readable text".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeCamera;

    fn device_info(firmware: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; 234];
        data[..4].copy_from_slice(b"17B0");
        data[33..37].copy_from_slice(b"033B");
        data[97..97 + firmware.len()].copy_from_slice(firmware);
        data
    }

    #[test]
    fn reads_firmware_version() {
        let fake =
            FakeCamera::new().selector(control::DEVICE_INFO, &device_info(b"v3.2.7.8_build2"));
        assert_eq!(fake.camera().firmware_version().unwrap(), "v3.2.7.8_build2");
    }

    #[test]
    fn rejects_short_or_garbled_device_information() {
        let short = FakeCamera::new().selector(control::DEVICE_INFO, &[0; 52]);
        assert!(matches!(
            short.camera().firmware_version(),
            Err(Error::Protocol(_))
        ));
        for firmware in [&b""[..], &[0xff, 0xfe]] {
            let fake = FakeCamera::new().selector(control::DEVICE_INFO, &device_info(firmware));
            assert!(matches!(
                fake.camera().firmware_version(),
                Err(Error::Protocol(_))
            ));
        }
    }

    #[test]
    fn reads_serial_number() {
        let mut sn = [0u8; 32];
        sn[..6].copy_from_slice(b"IBNLB2");
        let fake = FakeCamera::new().selector(control::SERIAL_NUMBER, &sn);
        assert_eq!(fake.camera().serial_number().unwrap(), "IBNLB2");
        assert!(matches!(
            FakeCamera::new().camera().serial_number(),
            Err(Error::Unsupported)
        ));
    }
}
