# Features and verification

Feature support and test results by camera model.

The crate detects features from the camera's reported capabilities. Other models may work but remain
untested. Reports for untested models are welcome.

| Status | Meaning |
|---|---|
| Verified | Effect confirmed in video, captured frames, or gimbal readings |
| Stored | Value accepted and read back, but effect unconfirmed |
| Ignored | Selector exists, but writes have no effect |
| Untested | Not tested on this model |
| No gimbal | Not applicable: this model has no gimbal |
| Unsupported | Not supported by the crate on this model |

## Gimbal

Position and zoom readings reflect changes from tracking, gestures, or manual movement where
supported.

| Feature | Link | Link 2C | Link 2 | Link 2C Pro | Link 2 Pro |
|---|---|---|---|---|---|
| Pan, tilt, center | Untested | No gimbal | Verified | No gimbal | Untested |
| Zoom | Untested | Untested | Verified | Untested | Untested |
| Actual position | Untested | No gimbal | Verified | No gimbal | Untested |
| Actual zoom | Untested | Untested | Verified | Untested | Untested |
| Privacy mode | Unsupported on `4c01`, untested on `4c02` | Untested | Verified | Untested | Untested |
| Presets: save, list, recall, delete | Untested | Untested | Verified | Untested | Untested |

## Image

| Feature | Link | Link 2C | Link 2 | Link 2C Pro | Link 2 Pro |
|---|---|---|---|---|---|
| Brightness, contrast, saturation, sharpness, hue | Untested | Untested | Verified | Untested | Untested |
| White balance, focus, anti-flicker | Untested | Untested | Verified | Untested | Untested |
| Automatic exposure with compensation | Untested | Untested | Verified | Untested | Untested |
| Manual ISO and shutter speed | Untested | Untested | Verified | Untested | Untested |

## Modes

| Feature | Link | Link 2C | Link 2 | Link 2C Pro | Link 2 Pro |
|---|---|---|---|---|---|
| Normal, tracking, overhead, DeskView | Untested | Untested | Verified | Untested | Untested |
| Whiteboard | Untested | Untested | Verified | Untested | Untested |
| Auto Framing | Untested | Untested | Stored (requires multiple people to verify) | Untested | Untested |
| Status: whiteboard search, detection, and failure, plus DeskView failure | Untested | Untested | Verified | Untested | Untested |

## Tracking

| Feature | Link | Link 2C | Link 2 | Link 2C Pro | Link 2 Pro |
|---|---|---|---|---|---|
| Framing: head, half body, whole body, manual | Untested | Untested | Verified | Untested | Untested |
| Tracking speed | Untested | Untested | Verified | Untested | Untested |
| Pause-track areas | Untested | Untested | Verified | Untested | Untested |
| Tracking area limits | Untested | No gimbal | Ignored | No gimbal | Untested |

## Switches

| Feature | Link | Link 2C | Link 2 | Link 2C Pro | Link 2 Pro |
|---|---|---|---|---|---|
| AI Zoom | Untested | Untested | Verified | Untested | Untested |
| HDR, horizontal flip, vertical flip | Untested | Untested | Verified | Untested | Untested |
| Enable or disable all hand gestures | Untested | Untested | Verified | Untested | Untested |
| Mute in privacy mode | Untested | Untested | Verified | Untested | Untested |
| Smart Adjustment, noise canceling | Untested | Untested | Stored (no observable effect) | Untested | Untested |
| Horizontal correction | Untested | Untested | Stored | Untested | Untested |

## Gestures

Each gesture can be enabled or disabled separately.

| Feature | Link | Link 2C | Link 2 | Link 2C Pro | Link 2 Pro |
|---|---|---|---|---|---|
| Tracking (palm), zoom (L), whiteboard (V) | Untested | Untested | Verified | Untested | Untested |

## Audio

| Feature | Link | Link 2C | Link 2 | Link 2C Pro | Link 2 Pro |
|---|---|---|---|---|---|
| Voice focus, voice suppression, music balance | Untested | Untested | Stored | Untested | Untested |

## Device

| Feature | Link | Link 2C | Link 2 | Link 2C Pro | Link 2 Pro |
|---|---|---|---|---|---|
| Firmware version, serial number, streaming status | Untested | Untested | Verified | Untested | Untested |
