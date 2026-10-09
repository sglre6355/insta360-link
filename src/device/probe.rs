//! Results of [`Camera::probe`](crate::Camera::probe).

use crate::device::xu::{Guid, UnitKind};

/// The capability flags a selector reports for `GET_INFO`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XuInfo(pub u8);

impl XuInfo {
    /// The selector can be read.
    pub fn can_get(&self) -> bool {
        self.0 & 0x01 != 0
    }

    /// The selector can be written.
    pub fn can_set(&self) -> bool {
        self.0 & 0x02 != 0
    }
}

/// Result of probing a selector.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProbeValue {
    /// The current value.
    Bytes(Vec<u8>),
    /// Reading was skipped because the selector is
    /// [blocked](crate::xu::is_blocked).
    Blocked,
    /// Reading was skipped because the unit is not a recognized Link unit.
    UnknownUnit,
    /// The selector reports no length or does not support reading.
    NotReadable,
    /// The camera rejected the read. Contains the system error message.
    Failed(String),
}

/// One selector of one extension unit, as seen by a probe.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SelectorProbe {
    /// Id of the unit the selector belongs to.
    pub unit_id: u8,
    /// GUID of that unit.
    pub guid: Guid,
    /// Which Link unit it is, if known.
    pub kind: Option<UnitKind>,
    /// The selector number.
    pub selector: u8,
    /// The selector's name, if it is a known one.
    pub name: Option<String>,
    /// Capability flags, if the camera answered `GET_INFO`.
    pub info: Option<XuInfo>,
    /// Payload length in bytes, if the camera answered `GET_LEN`.
    pub len: Option<u16>,
    /// The current value, or why it was not read.
    pub value: ProbeValue,
}
