//! A fake camera for unit tests.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::rc::Rc;

use crate::camera::Camera;
use crate::device::info::DeviceInfo;
use crate::device::selectors::Selector;
use crate::device::transport::{Control, ControlInfo, Transport, XuQuery};
use crate::device::xu::{ExtensionUnit, Guid, UnitKind};

#[derive(Default)]
struct State {
    controls: BTreeMap<Control, (ControlInfo, i32)>,
    units: Vec<ExtensionUnit>,
    selectors: BTreeMap<(u8, u8), Vec<u8>>,
    writes: Vec<Vec<(Control, i32)>>,
    xu_writes: Vec<(u8, u8, Vec<u8>)>,
    xu_reads: Vec<(u8, u8)>,
    swallow_xu_writes: bool,
    drop_xu_writes: usize,
    scripted: BTreeMap<(u8, u8), VecDeque<Vec<u8>>>,
}

impl State {
    /// The `(unit id, selector)` pair a selector is stored under.
    fn key(&self, selector: Selector) -> (u8, u8) {
        let unit = self
            .units
            .iter()
            .find(|unit| unit.kind == Some(selector.unit))
            .unwrap();
        (unit.id, selector.id)
    }
}

/// An in-memory device that remembers what was written to it.
///
/// Clones share state, so a test can keep one handle and give another to a
/// [`Camera`].
#[derive(Clone, Default)]
pub(crate) struct FakeCamera(Rc<RefCell<State>>);

/// The error for something the fake device does not have.
fn missing() -> io::Error {
    io::ErrorKind::NotFound.into()
}

impl FakeCamera {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Adds a standard control, starting at its default value.
    pub(crate) fn control(
        self,
        id: Control,
        minimum: i32,
        maximum: i32,
        step: i32,
        default: i32,
    ) -> Self {
        let info = ControlInfo {
            minimum,
            maximum,
            step,
            default,
        };
        self.0.borrow_mut().controls.insert(id, (info, default));
        self
    }

    /// Adds a selector to one of the Link units, creating the unit with a fixed
    /// id (9 for control, 10 for aux, 11 for AI) if it is not there yet.
    pub(crate) fn selector(self, selector: Selector, value: &[u8]) -> Self {
        let kind = selector.unit;
        let id = match kind {
            UnitKind::Control => 9,
            UnitKind::Aux => 10,
            UnitKind::Ai => 11,
        };
        self.raw_selector(id, kind.guid(), Some(kind), selector.id, value)
    }

    /// Adds a selector to a unit this crate does not recognize.
    pub(crate) fn unknown_selector(self, id: u8, selector: u8, value: &[u8]) -> Self {
        self.raw_selector(id, Guid([0x11; 16]), None, selector, value)
    }

    /// Advertises a selector in the descriptor without the device answering for
    /// it.
    pub(crate) fn dead_selector(self, selector: Selector) -> Self {
        let fake = self.selector(selector, &[]);
        fake.0
            .borrow_mut()
            .selectors
            .retain(|&(_, id), _| id != selector.id);
        fake
    }

    fn raw_selector(
        self,
        id: u8,
        guid: Guid,
        kind: Option<UnitKind>,
        selector: u8,
        value: &[u8],
    ) -> Self {
        {
            let mut state = self.0.borrow_mut();
            if !state.units.iter().any(|unit| unit.id == id) {
                state.units.push(ExtensionUnit {
                    id,
                    guid,
                    kind,
                    controls: vec![0; 4],
                });
            }
            let unit = state.units.iter_mut().find(|unit| unit.id == id).unwrap();
            let bit = selector - 1;
            unit.controls[usize::from(bit / 8)] |= 1 << (bit % 8);
            state.selectors.insert((id, selector), value.to_vec());
        }
        self
    }

    /// Makes the device accept selector writes without applying them, as a real
    /// camera does while it is not streaming.
    pub(crate) fn swallow_xu_writes(self) -> Self {
        self.0.borrow_mut().swallow_xu_writes = true;
        self
    }

    /// Makes the device ignore the next `count` selector writes, then behave
    /// normally again.
    pub(crate) fn drop_next_xu_writes(&self, count: usize) {
        self.0.borrow_mut().drop_xu_writes = count;
    }

    /// Overwrites a selector's value behind the camera's back.
    pub(crate) fn poke(&self, selector: Selector, value: &[u8]) {
        let mut state = self.0.borrow_mut();
        let key = state.key(selector);
        state.selectors.insert(key, value.to_vec());
    }

    /// Queues values to answer the next reads of a selector with, ahead of its
    /// stored value. Lets a test play back a camera changing state on its own.
    pub(crate) fn script<const N: usize>(&self, selector: Selector, values: [Vec<u8>; N]) {
        let mut state = self.0.borrow_mut();
        let key = state.key(selector);
        state.scripted.insert(key, values.into());
    }

    pub(crate) fn camera(&self) -> Camera {
        self.camera_with_product_id(0x4c04)
    }

    /// A camera that identifies itself as another model.
    pub(crate) fn camera_with_product_id(&self, product_id: u16) -> Camera {
        let units = self.0.borrow().units.clone();
        Camera::with_transport(
            DeviceInfo::new("/dev/video0", product_id, units),
            self.clone(),
        )
    }

    pub(crate) fn set(&self, id: Control, value: i32) {
        self.0.borrow_mut().controls.get_mut(&id).unwrap().1 = value;
    }

    pub(crate) fn get(&self, id: Control) -> i32 {
        self.0.borrow().controls[&id].1
    }

    /// Every control write so far, one entry per request.
    pub(crate) fn writes(&self) -> Vec<Vec<(Control, i32)>> {
        self.0.borrow().writes.clone()
    }

    /// Every selector write so far, as `(unit id, selector, payload)`.
    pub(crate) fn xu_writes(&self) -> Vec<(u8, u8, Vec<u8>)> {
        self.0.borrow().xu_writes.clone()
    }

    /// Every selector value read so far, as `(unit id, selector)`.
    pub(crate) fn xu_reads(&self) -> Vec<(u8, u8)> {
        self.0.borrow().xu_reads.clone()
    }
}

impl Transport for FakeCamera {
    fn xu_query(
        &self,
        unit_id: u8,
        selector: u8,
        query: XuQuery,
        data: &mut [u8],
    ) -> io::Result<()> {
        let mut state = self.0.borrow_mut();
        let key = (unit_id, selector);
        let len = state.selectors.get(&key).ok_or_else(missing)?.len();
        match query {
            XuQuery::GetInfo => data[0] = 0x03,
            XuQuery::GetLen => data.copy_from_slice(&(len as u16).to_le_bytes()),
            XuQuery::GetCur | XuQuery::SetCur if data.len() != len => {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            XuQuery::GetCur => {
                let scripted = state.scripted.get_mut(&key).and_then(VecDeque::pop_front);
                data.copy_from_slice(scripted.as_ref().unwrap_or(&state.selectors[&key]));
                state.xu_reads.push(key);
            }
            XuQuery::SetCur => {
                if state.drop_xu_writes > 0 {
                    state.drop_xu_writes -= 1;
                } else if !state.swallow_xu_writes {
                    state.selectors.insert(key, data.to_vec());
                }
                state.xu_writes.push((unit_id, selector, data.to_vec()));
            }
            _ => return Err(io::ErrorKind::Unsupported.into()),
        }
        Ok(())
    }

    fn query_control(&self, control: Control) -> io::Result<Option<ControlInfo>> {
        let state = self.0.borrow();
        Ok(state.controls.get(&control).map(|(info, _)| *info))
    }

    fn get_controls(&self, controls: &mut [(Control, i32)]) -> io::Result<()> {
        let state = self.0.borrow();
        for (id, value) in controls {
            *value = state.controls.get(id).ok_or_else(missing)?.1;
        }
        Ok(())
    }

    fn set_controls(&self, controls: &[(Control, i32)]) -> io::Result<()> {
        let mut state = self.0.borrow_mut();
        for (id, value) in controls {
            let (info, _) = state.controls.get(id).ok_or_else(missing)?;
            if !(info.minimum..=info.maximum).contains(value) {
                return Err(io::ErrorKind::InvalidInput.into());
            }
        }
        for &(id, value) in controls {
            state.controls.get_mut(&id).unwrap().1 = value;
        }
        state.writes.push(controls.to_vec());
        Ok(())
    }
}
