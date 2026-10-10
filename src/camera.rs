//! The [`Camera`] handle.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::device::backend;
use crate::device::info::{DeviceId, DeviceInfo};
use crate::device::probe::{ProbeValue, SelectorProbe, XuInfo};
use crate::device::selectors::{Selector, control};
use crate::device::transport::{Control, ControlInfo, Transport, XuQuery};
#[cfg(feature = "unsafe-raw")]
use crate::device::xu::UnitKind;
use crate::device::xu::{self, ExtensionUnit};
use crate::error::{Error, Result};

#[cfg(not(test))]
const SETTLE_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(not(test))]
const POLL_INTERVAL: Duration = Duration::from_millis(100);
#[cfg(test)]
const SETTLE_TIMEOUT: Duration = Duration::from_millis(30);
#[cfg(test)]
const POLL_INTERVAL: Duration = Duration::from_millis(1);

/// The shortest time between two vendor writes. The camera silently drops a
/// write that follows another within about 45 ms.
#[cfg(not(test))]
const WRITE_GAP: Duration = Duration::from_millis(100);
#[cfg(test)]
const WRITE_GAP: Duration = Duration::from_millis(2);

/// An open Insta360 Link camera.
///
/// Opening a camera does not start streaming and does not take it away from
/// other programs, so it can be controlled while a video call is using it.
pub struct Camera {
    transport: Box<dyn Transport>,
    info: DeviceInfo,
    /// When the last vendor write was sent. It starts at the moment the camera
    /// is opened, because another program may have written just before.
    last_write: Cell<Instant>,
    /// Payload lengths the camera has reported, by unit id and selector.
    lens: RefCell<HashMap<(u8, u8), u16>>,
}

impl Camera {
    /// Lists the connected Link cameras without opening them.
    pub fn list() -> Result<Vec<DeviceInfo>> {
        backend::list()
    }

    /// Opens the first connected Link camera.
    pub fn open_first() -> Result<Self> {
        let info = Self::list()?.into_iter().next().ok_or(Error::NotFound)?;
        Self::open_device(info)
    }

    /// Opens the camera with the given address, as found in [`DeviceInfo::id`].
    pub fn open(id: impl Into<DeviceId>) -> Result<Self> {
        Self::open_device(backend::find(&id.into())?)
    }

    fn open_device(info: DeviceInfo) -> Result<Self> {
        let transport = backend::open(&info)?;
        Ok(Self {
            transport,
            info,
            last_write: Cell::new(Instant::now()),
            lens: RefCell::default(),
        })
    }

    /// Wraps a transport supplied by the caller, for platforms without built-in
    /// support and for tests.
    pub fn with_transport(info: DeviceInfo, transport: impl Transport + 'static) -> Self {
        Self {
            transport: Box::new(transport),
            info,
            last_write: Cell::new(Instant::now()),
            lens: RefCell::default(),
        }
    }

    /// What is known about the device without talking to it.
    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    pub(crate) fn transport(&self) -> &dyn Transport {
        self.transport.as_ref()
    }

    /// Describes a standard control, or fails with [`Error::Unsupported`] if
    /// the camera lacks it.
    pub(crate) fn control_info(&self, control: Control) -> Result<ControlInfo> {
        self.transport
            .query_control(control)
            .map_err(|err| Error::io(format!("cannot query {control}"), err))?
            .ok_or(Error::Unsupported)
    }

    pub(crate) fn get_control(&self, control: Control) -> Result<i32> {
        self.control_info(control)?;
        let mut values = [(control, 0)];
        self.transport
            .get_controls(&mut values)
            .map_err(|err| Error::io(format!("cannot read {control}"), err))?;
        Ok(values[0].1)
    }

    pub(crate) fn set_control(&self, control: Control, value: i32) -> Result<()> {
        self.transport
            .set_controls(&[(control, value)])
            .map_err(|err| Error::io(format!("cannot set {control}"), err))
    }

    /// Reads the length, capability flags, and current value of every selector
    /// advertised by the camera's extension units.
    ///
    /// This operation never writes to the camera. For blocked selectors and
    /// selectors in unrecognized units, it queries only the length and
    /// capability flags, without reading their values. See [`xu::is_blocked`].
    pub fn probe(&self) -> Vec<SelectorProbe> {
        self.info
            .units
            .iter()
            .flat_map(|unit| {
                unit.selectors()
                    .map(move |selector| self.probe_selector(unit, selector))
            })
            .collect()
    }

    fn probe_selector(&self, unit: &ExtensionUnit, selector: u8) -> SelectorProbe {
        let mut byte = [0u8; 1];
        let info = self
            .transport
            .xu_query(unit.id, selector, XuQuery::GetInfo, &mut byte)
            .ok()
            .map(|()| XuInfo(byte[0]));
        let len = self.xu_len_raw(unit.id, selector).ok();

        let value = match (unit.kind, len) {
            (None, _) => ProbeValue::UnknownUnit,
            (Some(kind), _) if xu::is_blocked(kind, selector) => ProbeValue::Blocked,
            (Some(_), Some(len)) if len > 0 && info.is_none_or(|info| info.can_get()) => {
                let mut data = vec![0u8; usize::from(len)];
                match self
                    .transport
                    .xu_query(unit.id, selector, XuQuery::GetCur, &mut data)
                {
                    Ok(()) => ProbeValue::Bytes(data),
                    Err(err) => ProbeValue::Failed(err.to_string()),
                }
            }
            _ => ProbeValue::NotReadable,
        };

        SelectorProbe {
            unit_id: unit.id,
            guid: unit.guid,
            kind: unit.kind,
            selector,
            name: unit.kind.and_then(|kind| xu::selector_name(kind, selector)),
            info,
            len,
            value,
        }
    }

    fn xu_len_raw(&self, unit_id: u8, selector: u8) -> io::Result<u16> {
        let mut len = [0u8; 2];
        self.transport
            .xu_query(unit_id, selector, XuQuery::GetLen, &mut len)?;
        Ok(u16::from_le_bytes(len))
    }

    /// The payload length of a selector. A selector's length does not change
    /// while the camera is connected, so it is asked for once and remembered.
    fn xu_len(&self, unit_id: u8, selector: u8) -> io::Result<u16> {
        let key = (unit_id, selector);
        if let Some(&len) = self.lens.borrow().get(&key) {
            return Ok(len);
        }
        let len = self.xu_len_raw(unit_id, selector)?;
        self.lens.borrow_mut().insert(key, len);
        Ok(len)
    }

    /// Sends a request for a selector's value. A failure may mean the
    /// remembered length is wrong, so it is forgotten and asked for again next
    /// time.
    fn xu_value_query(
        &self,
        unit_id: u8,
        selector: u8,
        query: XuQuery,
        data: &mut [u8],
    ) -> io::Result<()> {
        let result = self.transport.xu_query(unit_id, selector, query, data);
        if result.is_err() {
            self.lens.borrow_mut().remove(&(unit_id, selector));
        }
        result
    }

    /// Finds the id of the selector's unit and checks that the selector may be
    /// used.
    fn unit_id(&self, selector: Selector) -> Result<u8> {
        if selector.is_blocked() {
            return Err(Error::Blocked {
                unit: selector.unit,
                selector: selector.id,
            });
        }
        self.info
            .unit(selector.unit)
            .filter(|unit| unit.has_selector(selector.id))
            .map(|unit| unit.id)
            .ok_or(Error::Unsupported)
    }

    /// Reads the current value of a selector, sized by what the camera reports
    /// for it.
    pub(crate) fn xu_read(&self, selector: Selector) -> Result<Vec<u8>> {
        let unit_id = self.unit_id(selector)?;
        let context = || format!("cannot read {selector}");
        let len = self
            .xu_len(unit_id, selector.id)
            .map_err(|err| Error::io(context(), err))?;
        let mut data = vec![0u8; usize::from(len)];
        self.xu_value_query(unit_id, selector.id, XuQuery::GetCur, &mut data)
            .map_err(|err| Error::io(context(), err))?;
        Ok(data)
    }

    /// Writes a selector. The payload must be exactly as long as the camera
    /// reports for it, which catches a firmware whose layout differs from the
    /// one this crate knows.
    pub(crate) fn xu_write(&self, selector: Selector, data: &[u8]) -> Result<()> {
        let unit_id = self.unit_id(selector)?;
        let context = || format!("cannot write {selector}");
        let len = self
            .xu_len(unit_id, selector.id)
            .map_err(|err| Error::io(context(), err))?;
        if usize::from(len) != data.len() {
            return Err(Error::Protocol(format!(
                "{selector} takes {len} bytes on this camera, not {}",
                data.len()
            )));
        }
        let mut data = data.to_vec();
        sleep(WRITE_GAP.saturating_sub(self.last_write.get().elapsed()));
        let written = self
            .xu_value_query(unit_id, selector.id, XuQuery::SetCur, &mut data)
            .map_err(|err| Error::io(context(), err));
        self.last_write.set(Instant::now());
        written
    }

    /// Whether some program is streaming video from the camera.
    ///
    /// # Errors
    ///
    /// Fails with [`Error::Unsupported`] on cameras that do not report it.
    pub fn is_streaming(&self) -> Result<bool> {
        let status = self.xu_read(control::DEVICE_STATUS)?;
        status.get(3).map(|&byte| byte != 0).ok_or_else(|| {
            Error::Protocol(format!("the device status is only {} bytes", status.len()))
        })
    }

    /// Rejects vendor writes that would be silently ignored when video is not
    /// streaming. Allows writes if the streaming state cannot be determined.
    pub(crate) fn require_streaming(&self) -> Result<()> {
        match self.is_streaming() {
            Ok(false) => Err(Error::NotStreaming),
            Ok(true) | Err(Error::Unsupported) => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// Writes a selector once and waits for `done`.
    ///
    /// The camera occasionally drops a write without reporting an error. That
    /// surfaces as [`Error::NotApplied`]. Whether to send the value again is
    /// left to the caller.
    pub(crate) fn write_verified(
        &self,
        selector: Selector,
        data: &[u8],
        done: impl Fn() -> Result<bool>,
    ) -> Result<()> {
        self.xu_write(selector, data)?;
        self.settle(done)
    }

    /// Polls until `done` reports true, or fails with [`Error::NotApplied`]
    /// after a few seconds.
    pub(crate) fn settle(&self, done: impl Fn() -> Result<bool>) -> Result<()> {
        self.poll_within(SETTLE_TIMEOUT, || Ok(done()?.then_some(())))?
            .ok_or(Error::NotApplied)
    }

    /// Polls until `check` produces a value, or returns `None` after `timeout`.
    pub(crate) fn poll_within<T>(
        &self,
        timeout: Duration,
        check: impl Fn() -> Result<Option<T>>,
    ) -> Result<Option<T>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(value) = check()? {
                return Ok(Some(value));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            sleep(POLL_INTERVAL);
        }
    }
}

/// Raw selector access for exploring features not yet modeled by this crate.
///
/// Incorrect writes may leave the camera in a state that requires unplugging
/// and reconnecting it. Selectors that could cause lasting damage remain
/// [blocked](xu::is_blocked).
#[cfg(feature = "unsafe-raw")]
impl Camera {
    /// Reads the current value of any selector that is not blocked.
    pub fn raw_read(&self, kind: UnitKind, selector: u8) -> Result<Vec<u8>> {
        self.xu_read(Selector::new(kind, selector))
    }

    /// Writes any selector that is not blocked. The payload must have the
    /// length the camera reports for it.
    pub fn raw_write(&self, kind: UnitKind, selector: u8, data: &[u8]) -> Result<()> {
        self.xu_write(Selector::new(kind, selector), data)
    }
}

impl std::fmt::Debug for Camera {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Camera").field("info", &self.info).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::selectors::ai;
    use crate::testing::FakeCamera;

    #[test]
    fn camera_can_move_to_another_thread() {
        fn assert_send<T: Send>() {}
        assert_send::<Camera>();
    }

    #[test]
    fn a_selector_is_asked_for_its_length_once() {
        let fake = FakeCamera::new()
            .selector(control::VIEW_STATE, &[0, 0])
            .selector(control::DEVICE_STATUS, &[0; 5]);
        let camera = fake.camera();
        camera.xu_read(control::VIEW_STATE).unwrap();
        camera.xu_write(control::VIEW_STATE, &[1, 0]).unwrap();
        camera.xu_read(control::VIEW_STATE).unwrap();
        camera.xu_read(control::DEVICE_STATUS).unwrap();
        assert_eq!(
            fake.xu_len_queries(),
            [(9, control::VIEW_STATE.id), (9, control::DEVICE_STATUS.id)]
        );
        // A probe reports what the camera says now.
        camera.probe();
        assert_eq!(fake.xu_len_queries().len(), 4);
    }

    #[test]
    fn a_failed_request_makes_the_length_be_asked_for_again() {
        let fake = FakeCamera::new().selector(control::VIEW_STATE, &[0, 0]);
        let camera = fake.camera();
        camera.xu_read(control::VIEW_STATE).unwrap();
        // The fake rejects a request of the wrong length, as the driver does.
        fake.poke(control::VIEW_STATE, &[1, 2, 3]);
        assert!(matches!(
            camera.xu_read(control::VIEW_STATE),
            Err(Error::Io { .. })
        ));
        assert_eq!(camera.xu_read(control::VIEW_STATE).unwrap(), [1, 2, 3]);
        assert_eq!(fake.xu_len_queries().len(), 2);
    }

    #[test]
    fn vendor_writes_are_spaced_apart() {
        let fake = FakeCamera::new().selector(control::VIEW_STATE, &[0, 0]);
        let start = Instant::now();
        let camera = fake.camera();
        // The first write waits too, counted from when the camera was
        // opened.
        camera.xu_write(control::VIEW_STATE, &[1, 0]).unwrap();
        assert!(start.elapsed() >= WRITE_GAP);
        camera.xu_write(control::VIEW_STATE, &[2, 0]).unwrap();
        assert!(start.elapsed() >= WRITE_GAP * 2);
        assert_eq!(fake.xu_writes().len(), 2);
    }

    #[test]
    fn probe_reads_ordinary_selectors() {
        let fake = FakeCamera::new()
            .selector(control::VIEW_STATE, &[1, 0])
            .selector(control::DEVICE_INFO, &[7]);
        let probes = fake.camera().probe();
        assert_eq!(probes.len(), 2);
        assert_eq!(probes[0].name.as_deref(), Some("view_state"));
        assert_eq!(probes[0].len, Some(2));
        assert_eq!(probes[0].value, ProbeValue::Bytes(vec![1, 0]));
        assert_eq!(probes[1].value, ProbeValue::Bytes(vec![7]));
    }

    #[test]
    fn probe_never_reads_blocked_or_unknown_units() {
        let fake = FakeCamera::new()
            .selector(control::FIRMWARE_UPGRADE_OR_BLEND_DRAW, &[0; 4])
            .unknown_selector(4, 1, &[0; 4]);
        let probes = fake.camera().probe();
        assert_eq!(probes[0].value, ProbeValue::Blocked);
        assert_eq!(probes[0].len, Some(4));
        assert_eq!(probes[1].value, ProbeValue::UnknownUnit);
        assert!(fake.xu_reads().is_empty());
    }

    #[test]
    fn probe_reports_selectors_the_camera_rejects() {
        let fake = FakeCamera::new().dead_selector(control::VIEW_STATE);
        let probes = fake.camera().probe();
        assert_eq!(probes[0].len, None);
        assert_eq!(probes[0].value, ProbeValue::NotReadable);
    }

    #[test]
    fn reads_refuse_blocked_and_missing_selectors() {
        let fake = FakeCamera::new()
            .selector(control::VIEW_STATE, &[5, 6, 7])
            .selector(control::FIRMWARE_UPGRADE_OR_BLEND_DRAW, &[0; 4]);
        let cam = fake.camera();
        assert_eq!(cam.xu_read(control::VIEW_STATE).unwrap(), [5, 6, 7]);
        assert!(matches!(
            cam.xu_read(control::FIRMWARE_UPGRADE_OR_BLEND_DRAW),
            Err(Error::Blocked { selector: 8, .. })
        ));
        assert!(matches!(
            cam.xu_read(control::DEVICE_INFO),
            Err(Error::Unsupported)
        ));
        assert!(matches!(cam.xu_read(ai::TRACKING), Err(Error::Unsupported)));
        assert_eq!(fake.xu_reads(), [(9, 2)]);
    }

    #[test]
    fn disabled_and_missing_controls_are_unsupported() {
        let fake = FakeCamera::new();
        assert!(matches!(
            fake.camera().get_control(Control::Zoom),
            Err(Error::Unsupported)
        ));
    }
}
