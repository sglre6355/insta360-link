//! Hardware communication, camera discovery, platform-specific access, and
//! vendor extension units.

pub(crate) mod identity;
pub(crate) mod info;
pub(crate) mod probe;
pub(crate) mod selectors;
pub mod transport;
pub(crate) mod view_state;
pub mod xu;

#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux as backend;

#[cfg(not(target_os = "linux"))]
pub(crate) mod unsupported;
#[cfg(not(target_os = "linux"))]
pub(crate) use unsupported as backend;
