//! The vendor extension units of the Link series.
//!
//! Link cameras expose their proprietary features through three UVC extension
//! units. Each is identified here by its GUID.
//!
//! Which selectors each unit has is listed in `selectors.rs`.

use std::fmt;

use super::selectors::Selector;

/// A UVC extension-unit GUID, in the byte order used on the wire.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Guid(pub [u8; 16]);

impl Guid {
    const fn from_fields(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> Self {
        let a = d1.to_le_bytes();
        let b = d2.to_le_bytes();
        let c = d3.to_le_bytes();
        Self([
            a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d4[0], d4[1], d4[2], d4[3], d4[4],
            d4[5], d4[6], d4[7],
        ])
    }
}

impl fmt::Display for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = &self.0;
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-",
            b[3], b[2], b[1], b[0], b[5], b[4], b[7], b[6], b[8], b[9]
        )?;
        b[10..].iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl fmt::Debug for Guid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Guid({self})")
    }
}

/// Which of the Link extension units a unit is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UnitKind {
    /// The main unit: modes, device information, exposure and feature switches.
    /// GUID `faf1672d-b71b-4793-8c91-7b1c9b7f95f8`.
    Control,
    /// The second unit: privacy mode, audio mode, pause-track areas and
    /// horizontal correction.
    /// GUID `e307e649-4618-a3ff-82fc-2d8b5f216773`.
    Aux,
    /// The AI unit: the tracking switch, presets and the tracking area.
    /// GUID `a8bd5df2-1a98-474e-8dd0-d92672d194fa`.
    Ai,
}

impl UnitKind {
    /// Every kind, in a stable order.
    pub const ALL: [Self; 3] = [Self::Control, Self::Aux, Self::Ai];

    /// The GUID that identifies this unit in the USB descriptors.
    pub const fn guid(&self) -> Guid {
        match self {
            Self::Control => Guid::from_fields(
                0xfaf1672d,
                0xb71b,
                0x4793,
                [0x8c, 0x91, 0x7b, 0x1c, 0x9b, 0x7f, 0x95, 0xf8],
            ),
            Self::Aux => Guid::from_fields(
                0xe307e649,
                0x4618,
                0xa3ff,
                [0x82, 0xfc, 0x2d, 0x8b, 0x5f, 0x21, 0x67, 0x73],
            ),
            Self::Ai => Guid::from_fields(
                0xa8bd5df2,
                0x1a98,
                0x474e,
                [0x8d, 0xd0, 0xd9, 0x26, 0x72, 0xd1, 0x94, 0xfa],
            ),
        }
    }

    /// Looks a GUID up among the known Link units.
    pub fn from_guid(guid: Guid) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.guid() == guid)
    }

    /// Short lowercase name, as used in text output.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Control => "control",
            Self::Aux => "aux",
            Self::Ai => "ai",
        }
    }
}

impl fmt::Display for UnitKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// An extension unit found in a device's USB descriptors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionUnit {
    /// The unit id requests are addressed to.
    pub id: u8,
    /// The GUID identifying what the unit is.
    pub guid: Guid,
    /// The Link unit this GUID corresponds to, if it is a known one.
    pub kind: Option<UnitKind>,
    /// Bitmap of the selectors the unit implements. Bit 0 is selector 1.
    pub controls: Vec<u8>,
}

impl ExtensionUnit {
    /// Whether the unit's descriptor advertises `selector`.
    pub fn has_selector(&self, selector: u8) -> bool {
        let Some(bit) = selector.checked_sub(1) else {
            return false;
        };
        self.controls
            .get(usize::from(bit / 8))
            .is_some_and(|byte| byte & (1 << (bit % 8)) != 0)
    }

    /// The selectors the unit's descriptor advertises, in ascending order.
    pub fn selectors(&self) -> impl Iterator<Item = u8> + '_ {
        (1..=u8::MAX).filter(|&selector| self.has_selector(selector))
    }
}

/// Extracts video-control extension units from a camera's USB descriptors,
/// provided as a single byte sequence containing the device descriptor followed
/// by the configuration descriptors.
///
/// Use this to obtain the extension units needed by
/// [`DeviceInfo::new`](crate::DeviceInfo::new) when using a custom transport.
pub fn parse_extension_units(descriptors: &[u8]) -> Vec<ExtensionUnit> {
    const DT_INTERFACE: u8 = 0x04;
    const DT_CS_INTERFACE: u8 = 0x24;
    const CLASS_VIDEO: u8 = 0x0e;
    const SUBCLASS_VIDEO_CONTROL: u8 = 0x01;
    const VC_EXTENSION_UNIT: u8 = 0x06;

    let mut units = Vec::new();
    let mut in_video_control = false;
    let mut rest = descriptors;
    while rest.len() >= 2 {
        let len = usize::from(rest[0]);
        if len < 2 || len > rest.len() {
            break;
        }
        let (desc, tail) = rest.split_at(len);
        rest = tail;

        match desc[1] {
            DT_INTERFACE if len >= 9 => {
                in_video_control = desc[5] == CLASS_VIDEO && desc[6] == SUBCLASS_VIDEO_CONTROL;
            }
            // Audio-control interfaces reuse subtype 0x06 for feature units, so
            // only look inside the video-control interface.
            DT_CS_INTERFACE if in_video_control && len >= 24 && desc[2] == VC_EXTENSION_UNIT => {
                let pins = usize::from(desc[21]);
                let Some(&control_size) = desc.get(22 + pins) else {
                    continue;
                };
                let start = 23 + pins;
                let Some(controls) = desc.get(start..start + usize::from(control_size)) else {
                    continue;
                };
                let guid = Guid(desc[4..20].try_into().expect("slice is 16 bytes"));
                units.push(ExtensionUnit {
                    id: desc[3],
                    guid,
                    kind: UnitKind::from_guid(guid),
                    controls: controls.to_vec(),
                });
            }
            _ => {}
        }
    }
    units
}

/// Returns the name of a selector, if known.
///
/// Selectors used by this crate have descriptive names such as `framing` and
/// `view_state`. Other selectors retain their vendor-defined names.
pub fn selector_name(kind: UnitKind, selector: u8) -> Option<String> {
    Selector::new(kind, selector).name()
}

/// Returns whether this crate blocks access to a selector.
///
/// Based on their vendor names, blocked selectors handle script execution, file
/// transfers, firmware flashing, USB mode switching, or licenses and keys.
/// Incorrect writes could render the camera unusable, so writes are prohibited.
/// Reads are also blocked as a precaution because their effects on
/// file-transfer selectors have not been investigated.
///
/// Only reads and writes of the selector's value are blocked.
/// [`Camera::probe`](crate::Camera::probe) still queries the length and
/// capability flags of a blocked selector.
pub fn is_blocked(kind: UnitKind, selector: u8) -> bool {
    Selector::new(kind, selector).is_blocked()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A descriptor blob with a video-control interface holding one known unit,
    /// followed by an audio-control interface holding a feature unit that must
    /// be ignored.
    pub(crate) fn sample_descriptors() -> Vec<u8> {
        let mut blob = vec![18, 0x01];
        blob.resize(18, 0);
        blob.extend_from_slice(&[9, 0x04, 0, 0, 1, 0x0e, 0x01, 0, 0]);
        let mut unit = vec![0, 0x24, 0x06, 9];
        unit.extend_from_slice(&UnitKind::Control.guid().0);
        unit.extend_from_slice(&[30, 1, 4, 4, 0x02, 0x00, 0x00, 0x20, 0]);
        unit[0] = unit.len() as u8;
        blob.extend_from_slice(&unit);
        blob.extend_from_slice(&[9, 0x04, 2, 0, 0, 0x01, 0x01, 0, 0]);
        let mut feature = vec![0, 0x24, 0x06, 9];
        feature.resize(26, 0xff);
        feature[0] = feature.len() as u8;
        blob.extend_from_slice(&feature);
        blob
    }

    #[test]
    fn guid_displays_like_lsusb() {
        assert_eq!(
            UnitKind::Control.guid().to_string(),
            "faf1672d-b71b-4793-8c91-7b1c9b7f95f8"
        );
        assert_eq!(
            UnitKind::Aux.guid().to_string(),
            "e307e649-4618-a3ff-82fc-2d8b5f216773"
        );
        assert_eq!(
            UnitKind::Ai.guid().to_string(),
            "a8bd5df2-1a98-474e-8dd0-d92672d194fa"
        );
    }

    #[test]
    fn parses_only_video_control_units() {
        let units = parse_extension_units(&sample_descriptors());
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].id, 9);
        assert_eq!(units[0].kind, Some(UnitKind::Control));
        assert_eq!(units[0].selectors().collect::<Vec<_>>(), [2, 30]);
    }

    #[test]
    fn tolerates_truncated_input() {
        let blob = sample_descriptors();
        for len in 0..blob.len() {
            parse_extension_units(&blob[..len]);
        }
        assert!(parse_extension_units(&[0, 0, 0]).is_empty());
    }
}
