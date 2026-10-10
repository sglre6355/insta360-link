//! Raw Linux kernel interfaces for V4L2 control and `uvcvideo` extension-unit
//! query ioctls.
//!
//! The structures mirror `linux/videodev2.h` and `linux/uvcvideo.h`. The
//! request numbers use the generic `_IOC` encoding shared by x86, ARM and
//! RISC-V.

use std::io;
use std::mem::size_of;
use std::os::fd::{AsRawFd, BorrowedFd};

const IOC_WRITE: u64 = 1;
const IOC_READ: u64 = 2;

const fn ioc(dir: u64, ty: u8, nr: u8, size: usize) -> u64 {
    (dir << 30) | ((size as u64) << 16) | ((ty as u64) << 8) | nr as u64
}

const fn iowr<T>(ty: u8, nr: u8) -> u64 {
    ioc(IOC_READ | IOC_WRITE, ty, nr, size_of::<T>())
}

/// `struct uvc_xu_control_query`
#[repr(C)]
struct UvcXuControlQuery {
    unit: u8,
    selector: u8,
    query: u8,
    size: u16,
    data: *mut u8,
}

/// `struct v4l2_queryctrl`
#[repr(C)]
struct V4l2Queryctrl {
    id: u32,
    type_: u32,
    name: [u8; 32],
    minimum: i32,
    maximum: i32,
    step: i32,
    default_value: i32,
    flags: u32,
    reserved: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
union V4l2ExtControlValue {
    value: i32,
    value64: i64,
}

/// `struct v4l2_ext_control`
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct V4l2ExtControl {
    id: u32,
    size: u32,
    reserved2: [u32; 1],
    value: V4l2ExtControlValue,
}

/// `struct v4l2_ext_controls`
#[repr(C)]
struct V4l2ExtControls {
    which: u32,
    count: u32,
    error_idx: u32,
    request_fd: i32,
    reserved: [u32; 1],
    controls: *mut V4l2ExtControl,
}

#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(size_of::<UvcXuControlQuery>() == 16);
    assert!(size_of::<V4l2Queryctrl>() == 68);
    assert!(size_of::<V4l2ExtControl>() == 20);
    assert!(size_of::<V4l2ExtControls>() == 32);
};

const UVCIOC_CTRL_QUERY: u64 = iowr::<UvcXuControlQuery>(b'u', 0x21);
const VIDIOC_QUERYCTRL: u64 = iowr::<V4l2Queryctrl>(b'V', 36);
const VIDIOC_G_EXT_CTRLS: u64 = iowr::<V4l2ExtControls>(b'V', 71);
const VIDIOC_S_EXT_CTRLS: u64 = iowr::<V4l2ExtControls>(b'V', 72);

const V4L2_CTRL_WHICH_CUR_VAL: u32 = 0;

/// Range and flags of a V4L2 control, as reported by `VIDIOC_QUERYCTRL`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RawControlInfo {
    pub minimum: i32,
    pub maximum: i32,
    pub step: i32,
    pub default: i32,
    pub flags: u32,
}

/// Runs an ioctl, retrying when interrupted by a signal.
///
/// # Safety
///
/// `arg` must point to the structure `request` expects, and every pointer
/// inside that structure must stay valid for the duration of the call.
unsafe fn ioctl<T>(fd: BorrowedFd<'_>, request: u64, arg: *mut T) -> io::Result<()> {
    loop {
        // SAFETY: guaranteed by the caller.
        let ret = unsafe { libc::ioctl(fd.as_raw_fd(), request as libc::Ioctl, arg) };
        if ret != -1 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

/// Sends one UVC request to an extension unit through `uvcvideo`.
pub(crate) fn xu_query(
    fd: BorrowedFd<'_>,
    unit: u8,
    selector: u8,
    query: u8,
    data: &mut [u8],
) -> io::Result<()> {
    let size = u16::try_from(data.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "payload exceeds 65535 bytes"))?;
    let mut request = UvcXuControlQuery {
        unit,
        selector,
        query,
        size,
        data: data.as_mut_ptr(),
    };
    // SAFETY: `request` matches UVCIOC_CTRL_QUERY and `data` outlives the call.
    unsafe { ioctl(fd, UVCIOC_CTRL_QUERY, &mut request) }
}

pub(crate) fn query_control(fd: BorrowedFd<'_>, id: u32) -> io::Result<RawControlInfo> {
    // SAFETY: an all-zero v4l2_queryctrl is a valid value.
    let mut request: V4l2Queryctrl = unsafe { std::mem::zeroed() };
    request.id = id;
    // SAFETY: `request` matches VIDIOC_QUERYCTRL.
    unsafe { ioctl(fd, VIDIOC_QUERYCTRL, &mut request)? };
    Ok(RawControlInfo {
        minimum: request.minimum,
        maximum: request.maximum,
        step: request.step,
        default: request.default_value,
        flags: request.flags,
    })
}

fn ext_controls(
    fd: BorrowedFd<'_>,
    request: u64,
    controls: &mut [V4l2ExtControl],
) -> io::Result<()> {
    let mut arg = V4l2ExtControls {
        which: V4L2_CTRL_WHICH_CUR_VAL,
        count: controls.len() as u32,
        error_idx: 0,
        request_fd: 0,
        reserved: [0],
        controls: controls.as_mut_ptr(),
    };
    // SAFETY: `arg` matches VIDIOC_[GS]_EXT_CTRLS and `controls` outlives the
    // call.
    unsafe { ioctl(fd, request, &mut arg) }
}

fn ext_control(id: u32, value: i32) -> V4l2ExtControl {
    // Zero the union before setting the 32-bit integer member read by the kernel.
    // The members overlap only on little-endian targets.
    let mut raw = V4l2ExtControlValue { value64: 0 };
    raw.value = value;
    V4l2ExtControl {
        id,
        size: 0,
        reserved2: [0],
        value: raw,
    }
}

/// Reads several controls in one request. `controls` holds `(id, value)`.
pub(crate) fn get_controls(fd: BorrowedFd<'_>, controls: &mut [(u32, i32)]) -> io::Result<()> {
    let mut raw: Vec<_> = controls.iter().map(|&(id, _)| ext_control(id, 0)).collect();
    ext_controls(fd, VIDIOC_G_EXT_CTRLS, &mut raw)?;
    for (control, raw) in controls.iter_mut().zip(&raw) {
        let value = raw.value;
        // SAFETY: every bit pattern is a valid i32.
        control.1 = unsafe { value.value };
    }
    Ok(())
}

/// Writes several controls in one request, so the driver commits them together.
pub(crate) fn set_controls(fd: BorrowedFd<'_>, controls: &[(u32, i32)]) -> io::Result<()> {
    let mut raw: Vec<_> = controls
        .iter()
        .map(|&(id, value)| ext_control(id, value))
        .collect();
    ext_controls(fd, VIDIOC_S_EXT_CTRLS, &mut raw)
}
