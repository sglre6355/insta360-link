//! Finding Link cameras through sysfs.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::device::info::{DeviceId, DeviceInfo};
use crate::device::xu::{self, UnitKind};
use crate::error::{Error, Result};
use crate::model::{Model, VENDOR_ID};

const SYS_CLASS: &str = "/sys/class/video4linux";
const DEV: &str = "/dev";

/// Lists the connected Link cameras, ordered by device node.
pub(crate) fn list() -> Result<Vec<DeviceInfo>> {
    list_in(Path::new(SYS_CLASS), Path::new(DEV))
}

/// Describes the camera behind a device node, following symlinks such as those
/// in `/dev/v4l/by-id`.
pub(crate) fn find(id: &DeviceId) -> Result<DeviceInfo> {
    let node = fs::canonicalize(id.as_str())
        .map_err(|err| Error::io(format!("cannot resolve {id}"), err))?;
    let not_a_link = || Error::NotALink(id.clone());
    let name = node.file_name().ok_or_else(not_a_link)?;
    inspect(&Path::new(SYS_CLASS).join(name), &node)
        .ok()
        .flatten()
        .ok_or_else(not_a_link)
}

fn list_in(sys_class: &Path, dev: &Path) -> Result<Vec<DeviceInfo>> {
    let entries = match fs::read_dir(sys_class) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(Error::io(
                format!("cannot read {}", sys_class.display()),
                err,
            ));
        }
    };
    let mut devices: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            // A node that vanishes or cannot be read mid-scan is skipped.
            let node = dev.join(entry.file_name());
            let device = inspect(&entry.path(), &node).ok().flatten()?;
            Some((node, device))
        })
        .collect();
    devices.sort_by_key(|(node, _)| node_number(node));
    Ok(devices.into_iter().map(|(_, device)| device).collect())
}

/// Returns the camera a sysfs `video4linux` entry belongs to, or `None` if the
/// entry is not the control node of a Link camera.
fn inspect(entry: &Path, node: &Path) -> io::Result<Option<DeviceInfo>> {
    // Each camera registers a capture node (index 0) and a metadata node.
    if read_trimmed(&entry.join("index"))? != "0" {
        return Ok(None);
    }
    let interface = fs::canonicalize(entry.join("device"))?;
    let Some(usb) = interface.parent() else {
        return Ok(None);
    };
    let (Some(vendor_id), Some(product_id)) = (
        read_hex(&usb.join("idVendor")),
        read_hex(&usb.join("idProduct")),
    ) else {
        return Ok(None);
    };
    if vendor_id != VENDOR_ID {
        return Ok(None);
    }

    let units = xu::parse_extension_units(&fs::read(usb.join("descriptors"))?);
    let model = Model::from_product_id(product_id);
    // Insta360's action cameras share the vendor id in webcam mode, so an
    // unrecognized product only counts if it has the Link control unit.
    let has_control_unit = units
        .iter()
        .any(|unit| unit.kind == Some(UnitKind::Control));
    if matches!(model, Model::Unknown(_)) && !has_control_unit {
        return Ok(None);
    }

    Ok(Some(DeviceInfo {
        id: DeviceId::new(node.to_string_lossy()),
        model,
        vendor_id,
        product_id,
        product: read_trimmed(&usb.join("product")).ok(),
        units,
    }))
}

fn read_trimmed(path: &Path) -> io::Result<String> {
    Ok(fs::read_to_string(path)?.trim().to_owned())
}

fn read_hex(path: &Path) -> Option<u16> {
    u16::from_str_radix(&read_trimmed(path).ok()?, 16).ok()
}

fn node_number(path: &Path) -> (u32, PathBuf) {
    let number = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| {
            name.trim_start_matches(|c: char| !c.is_ascii_digit())
                .parse()
                .ok()
        })
        .unwrap_or(u32::MAX);
    (number, path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    /// A throwaway fake sysfs tree, removed on drop.
    struct FakeSys(PathBuf);

    impl FakeSys {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("insta360-link-test-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("class")).unwrap();
            Self(root)
        }

        fn add(&self, video: &str, index: &str, vendor: &str, product: &str, descriptors: &[u8]) {
            let usb = self.0.join("usb").join(video);
            let interface = usb.join("iface");
            fs::create_dir_all(&interface).unwrap();
            fs::write(usb.join("idVendor"), format!("{vendor}\n")).unwrap();
            fs::write(usb.join("idProduct"), format!("{product}\n")).unwrap();
            fs::write(usb.join("product"), "Test Camera\n").unwrap();
            fs::write(usb.join("descriptors"), descriptors).unwrap();
            let entry = self.0.join("class").join(video);
            fs::create_dir_all(&entry).unwrap();
            fs::write(entry.join("index"), format!("{index}\n")).unwrap();
            symlink(&interface, entry.join("device")).unwrap();
        }

        fn list(&self) -> Vec<DeviceInfo> {
            list_in(&self.0.join("class"), Path::new("/dev")).unwrap()
        }
    }

    impl Drop for FakeSys {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn link_descriptors() -> Vec<u8> {
        crate::device::xu::tests::sample_descriptors()
    }

    #[test]
    fn finds_control_nodes_in_order() {
        let sys = FakeSys::new("order");
        sys.add("video10", "0", "2e1a", "4c04", &link_descriptors());
        sys.add("video11", "1", "2e1a", "4c04", &link_descriptors());
        sys.add("video2", "0", "2e1a", "4c01", &link_descriptors());
        let found = sys.list();
        let ids: Vec<_> = found.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["/dev/video2", "/dev/video10"]);
        assert_eq!(found[0].model, Model::Link);
        assert_eq!(found[1].model, Model::Link2);
        assert_eq!(found[1].product.as_deref(), Some("Test Camera"));
        assert!(found[1].unit(UnitKind::Control).is_some());
        assert!(found[1].unit(UnitKind::Ai).is_none());
    }

    #[test]
    fn ignores_other_vendors_and_unrelated_insta360_devices() {
        let sys = FakeSys::new("filter");
        sys.add("video0", "0", "046d", "4c04", &link_descriptors());
        sys.add("video1", "0", "2e1a", "0002", &[18, 1]);
        assert!(sys.list().is_empty());
    }

    #[test]
    fn accepts_unknown_product_with_the_control_unit() {
        let sys = FakeSys::new("unknown");
        sys.add("video0", "0", "2e1a", "4c7f", &link_descriptors());
        assert_eq!(sys.list()[0].model, Model::Unknown(0x4c7f));
    }

    #[test]
    fn missing_sysfs_means_no_cameras() {
        let found = list_in(Path::new("/nonexistent/video4linux"), Path::new("/dev")).unwrap();
        assert!(found.is_empty());
    }
}
