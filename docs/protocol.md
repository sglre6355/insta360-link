# Protocol notes

Use these protocol details to extend the crate or build another tool. For application-level
behavior, see [Behavior and limitations](behavior.md).

## Extension units

Units are identified by GUID, not by unit ID.

| Unit | GUID |
|---|---|
| Control | `faf1672d-b71b-4793-8c91-7b1c9b7f95f8` |
| Aux | `e307e649-4618-a3ff-82fc-2d8b5f216773` |
| AI | `a8bd5df2-1a98-474e-8dd0-d92672d194fa` |

## Selectors

### Control

| Selector | Bytes | Meaning |
|---|---|---|
| 2 | 61 | View state (`video_mode` in Link Controller). Byte 0: mode (0 normal, 1 tracking, 4 whiteboard, 5 overhead, 6 DeskView, 7 Auto Framing, `0xff` during transitions). Byte 1: mode status. Bytes 50–51: actual zoom in hundredths. Byte 54, bit 0: privacy mode |
| 3 | 234 | Device information, with the firmware version string at offset 97 |
| 5 | 1 | Gesture bits: 1 palm (tracking), 2 L (zoom), 3 V (whiteboard) |
| 7 | 1 | Noise canceling |
| 9 | 2 | Exposure compensation, signed hundredths of a stop |
| 11 | 5 | Status: byte 3 is nonzero while streaming |
| 12 | 32 | Serial number |
| 18 | 1 | Tracking speed: 0 fast, 1 normal, 2 slow |
| 19 | 1 | Framing: 1 head, 2 half body, 3 whole body, 4 manual |
| 25 | 2 | ISO |
| 26 | 8 | Measured tilt, then pan, each a little-endian `i32` in arcseconds |
| 27 | 2 | Feature bits: 0 AI Zoom, 2 HDR, 3 horizontal flip, 4 hand gestures, 9 Smart Adjustment, 11 mute in privacy, 12 vertical flip, 14 pause-track areas enabled |
| 29 | 2 | Shutter speed: denominator of a fraction of a second |
| 30 | 1 | Exposure mode: 1 manual, 2 automatic |

Changing feature bits 5 or 13 restarts the camera. A write to selector 27 must leave every bit it
does not mean to change as the camera has it.

### AI

| Selector | Bytes | Meaning |
|---|---|---|
| 2 | 1 | Tracking switch |
| 28 | 127 | Preset: slot, in-use flag, and 64-byte name. Mode at offset 66. Pan and tilt as `i32` tenths of a degree at 107 and 111. Zoom as `u16` hundredths at 115. Write slot + 100 to select the slot for the next read |
| 31 | 19 | Tracking area: enable flag, second flag, then `i16` angles in tenths of a degree: pan min, pan max, tilt max, tilt min, followed by four portrait-orientation limits |

### Aux

| Selector | Bytes | Meaning |
|---|---|---|
| 15 | 2 | Privacy: write 1 to enable or 0 to disable, as Link Controller does. Always reads back as 2 |
| 21 | 90 | Pause-track areas: six 15-byte records. Byte 0: zone number, bit 7 set if active. Bytes 3–6: pan and tilt as `i16` tenths of a degree. Bytes 7–14: x, y, width, height as `u16` ten-thousandths of the frame |
| 20 | 1 | Audio mode: 0 voice suppression, 1 voice focus, 2 music balance |
| 28 | 25 | Horizontal correction in byte 0 |

### Blocked

The crate never reads or writes these control-unit selectors, including through the raw API:

| Selector | Vendor name |
|---|---|
| 1 | `exec_script` |
| 8 | `firmware_upgrade`, `blend_draw` |
| 13 | `device_license` |
| 15 | `download_file`, `af_mode` |
| 16 | `upload_file`, `exposure_curve` |
| 17 | `usb_mode_switch` |
| 23 | `mobvoi_pubkey` |

## View state

View-state length varies by camera. Link Controller queries the length before reading position
fields (at least 52 bytes) or flags (at least 55 bytes). Link 2 returns 61 bytes. Other models have
not been observed.

Byte 0 is `0xff` during mode transitions. Byte 1 reports status and ignores writes:

| Value | Status |
|---|---|
| 1 | Searching for a whiteboard |
| 2 | Whiteboard found |
| 3 | Whiteboard not found |
| `0x10` | Observed during DeskView and briefly afterward on Link 2 |
| `0x11` | DeskView startup failure, only while byte 0 is 6 (DeskView) |

## Mode requests

Mode requests include a gimbal target starting at byte 38:

| Offset | Type | Field |
|---|---|---|
| 38 | `i32` | Pan |
| 42 | `i32` | Tilt |
| 46 | `i32` | Third angle |
| 50 | `u16` | Zoom |
| 52 | `i16` | Host pitch |
| 54 | `u16` | Flags |

Like Link Controller, the crate sends 3610 for each angle, no zoom, and a pitch of -450 to let the
camera choose. Link 2C models replace pan with two `i16` values of -1. The original Link leaves that
field unused. Only the Link 2 layout has been tested on hardware.
