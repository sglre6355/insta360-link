# insta360-link

Control Insta360 Link series webcams from Rust.

This crate uses the operating system's camera driver and can change settings while another program
streams video. Built-in support is currently limited to Linux. See [Platforms](#platforms).

This unofficial project is not affiliated with Insta360. The protocol was reverse-engineered from
Insta360's Link Controller application and camera behavior.

```rust
use insta360_link::{Camera, CameraMode, PanTiltUpdate};

let camera = Camera::open_first()?;
println!("{} running {}", camera.info().model, camera.firmware_version()?);

// Move one axis while preserving the other.
camera.set_position(PanTiltUpdate { pan: Some(20.0), tilt: None })?;
camera.set_zoom(1.5)?;

// Changing modes requires an active video stream.
camera.set_mode(CameraMode::Tracking)?;
```

## Features

- Gimbal: pan, tilt, zoom, privacy mode, and on-camera presets.
- Image: brightness, contrast, saturation, sharpness, hue, white balance, focus, anti-flicker, and
  automatic or manual exposure.
- Modes: normal, tracking, whiteboard, overhead, DeskView, and Auto Framing, plus mode status.
- Tracking: framing, tracking speed, pause-track areas, and tracking area limits.
- Switches: AI Zoom, HDR, flips, hand gestures, Smart Adjustment, mute in privacy, noise canceling,
  and horizontal correction.
- Audio: microphone mode.
- Device: firmware version, serial number, and streaming status.
- State: read and apply settings, and save or load them in any `serde` format.

Feature support and verification status vary by model. See
[Features and verification](docs/features.md) for details.

Exposure curves and portrait resolution are not supported. Virtual camera, beauty filters, and
backgrounds are features of Insta360's desktop software, not the camera.

## Supported devices

| Model | USB ID |
|---|---|
| Link | `2e1a:4c01`, `2e1a:4c02` |
| Link 2C | `2e1a:4c03` |
| Link 2 | `2e1a:4c04` |
| Link 2C Pro | `2e1a:4c05` |
| Link 2 Pro | `2e1a:4c06` |

Other Insta360 devices, including future models, may open as `Model::Unknown` if they expose the
Link control unit.

Features are detected from the camera's reported capabilities. Missing controls or selectors return
`Error::Unsupported`, allowing models without a gimbal, for example, to use other features.

## Platforms

Platform backends are separate from the protocol, which uses the `Transport` trait for standard
controls and extension-unit requests. The crate compiles on platforms without a built-in backend,
but `Camera::list` returns no devices there. Use `Camera::with_transport` to supply a custom
transport.

### Linux

Linux support uses the kernel's `uvcvideo` driver and requires only access to the camera's
`/dev/video*` node.

## Before you start

Keep these behaviors in mind:

- Vendor settings require an active video stream. Modes, framing, exposure, and switches fail with
  `Error::NotStreaming` unless some program is streaming from the camera. Gimbal movement, zoom, and
  standard picture settings work without one.
- Vendor writes are verified. The crate reads each value back and returns `Error::NotApplied` if the
  camera does not apply it. Writes are not retried automatically.
- `Camera::mode` can return `Error::Busy` briefly during a mode change, or while tracking has no
  person to follow.

See [Behavior and limitations](docs/behavior.md) for details and additional limitations.

## Safety

This crate aims to block potentially dangerous camera operations. It never reads or writes the
values of seven control-unit selectors that run scripts, transfer files, flash firmware, change USB
mode, or store licenses and keys. These restrictions also apply to the raw API. `Camera::probe`
still queries their length and capability flags without reading or writing their values.

Changing feature bits 5 or 13 restarts the camera. The typed API preserves unmodeled bits, but the
raw API can change them. Use care when writing the feature selector.

## Cargo features

- `serde`: implements `Serialize` and `Deserialize` for `State` and its settings, supporting formats
  such as TOML and JSON. Absent settings are omitted during serialization and left unchanged when
  applying state.
- `unsafe-raw`: adds `Camera::raw_read` and `Camera::raw_write` for selectors without a typed API.
  Incorrect writes may require unplugging and reconnecting the camera to recover.

## License

MIT. See [LICENSE](LICENSE).
