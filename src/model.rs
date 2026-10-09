//! Camera models and their USB identifiers.

use std::fmt;

/// USB vendor id of Insta360 (Arashi Vision).
pub const VENDOR_ID: u16 = 0x2e1a;

/// A camera in the Insta360 Link series.
///
/// Features are detected from what each device reports, not from this table, so
/// an unknown model is still usable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Model {
    /// Insta360 Link (`4c01` or `4c02`).
    Link,
    /// Insta360 Link 2C (`4c03`), the Link 2 without a gimbal.
    Link2C,
    /// Insta360 Link 2 (`4c04`).
    Link2,
    /// Insta360 Link 2C Pro (`4c05`), the Link 2 Pro without a gimbal.
    Link2CPro,
    /// Insta360 Link 2 Pro (`4c06`).
    Link2Pro,
    /// A device that carries the Link control interface but whose product id
    /// this crate does not know yet.
    Unknown(u16),
}

impl Model {
    /// Identifies a model from its USB product id.
    pub fn from_product_id(product_id: u16) -> Self {
        match product_id {
            0x4c01 | 0x4c02 => Self::Link,
            0x4c03 => Self::Link2C,
            0x4c04 => Self::Link2,
            0x4c05 => Self::Link2CPro,
            0x4c06 => Self::Link2Pro,
            other => Self::Unknown(other),
        }
    }

    /// Marketing name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Link => "Insta360 Link",
            Self::Link2C => "Insta360 Link 2C",
            Self::Link2 => "Insta360 Link 2",
            Self::Link2CPro => "Insta360 Link 2C Pro",
            Self::Link2Pro => "Insta360 Link 2 Pro",
            Self::Unknown(_) => "Insta360 Link series unknown model",
        }
    }
}

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(pid) => write!(f, "{} [{pid:04x}]", self.name()),
            _ => f.write_str(self.name()),
        }
    }
}
