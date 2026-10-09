//! Linux backend for discovering cameras through sysfs and communicating
//! with them through V4L2 device nodes created by the `uvcvideo` driver.

mod sys;
mod sysfs;
mod v4l2;

pub(crate) use sysfs::{find, list};
pub(crate) use v4l2::open;
