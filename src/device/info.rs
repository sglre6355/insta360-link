//! Camera information available before opening the device.

use std::fmt;

use super::xu::{ExtensionUnit, UnitKind};
use crate::model::{Model, VENDOR_ID};

/// Platform-specific identifier used to address a camera.
///
/// On Linux, this is the device node path, such as `/dev/video0`. Treat it as
/// opaque and pass it to [`Camera::open`](crate::Camera::open) or display it to
/// the user.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeviceId(String);

impl DeviceId {
    /// Wraps a platform address.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The address as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for DeviceId {
    fn from(id: &str) -> Self {
        Self::new(id)
    }
}

impl From<String> for DeviceId {
    fn from(id: String) -> Self {
        Self::new(id)
    }
}

/// A Link camera found on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeviceInfo {
    /// The address the camera is opened by.
    pub id: DeviceId,
    /// The model, derived from the USB product id.
    pub model: Model,
    /// USB vendor id.
    pub vendor_id: u16,
    /// USB product id.
    pub product_id: u16,
    /// The product string the camera reports, if any.
    pub product: Option<String>,
    /// The extension units listed in the camera's USB descriptors.
    pub units: Vec<ExtensionUnit>,
}

impl DeviceInfo {
    /// Describes a device by hand, for use with
    /// [`Camera::with_transport`](crate::Camera::with_transport).
    pub fn new(id: impl Into<DeviceId>, product_id: u16, units: Vec<ExtensionUnit>) -> Self {
        Self {
            id: id.into(),
            model: Model::from_product_id(product_id),
            vendor_id: VENDOR_ID,
            product_id,
            product: None,
            units,
        }
    }

    /// The extension unit of the given kind, if the camera has it.
    pub fn unit(&self, kind: UnitKind) -> Option<&ExtensionUnit> {
        self.units.iter().find(|unit| unit.kind == Some(kind))
    }
}
