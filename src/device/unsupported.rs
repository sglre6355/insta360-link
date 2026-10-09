//! The backend for platforms this crate cannot reach cameras on yet.
//!
//! [`Camera::with_transport`](crate::Camera::with_transport) still works there
//! with a transport supplied by the caller.

use super::info::{DeviceId, DeviceInfo};
use super::transport::Transport;
use crate::error::{Error, Result};

pub(crate) fn list() -> Result<Vec<DeviceInfo>> {
    Ok(Vec::new())
}

pub(crate) fn find(_id: &DeviceId) -> Result<DeviceInfo> {
    Err(Error::NotFound)
}

pub(crate) fn open(_info: &DeviceInfo) -> Result<Box<dyn Transport>> {
    Err(Error::Unsupported)
}
