# Behavior and limitations

This guide describes camera behavior that affects applications using this crate. For byte-level
details, see [Protocol notes](protocol.md).

## Vendor settings require an active video stream

The camera silently discards mode, framing, exposure, and switch changes unless a program is
streaming video. The crate checks for an active stream first and returns `Error::NotStreaming` if
none is running. Gimbal movement, zoom, and standard picture settings work without a stream.

## Vendor writes are verified

After a vendor write, the crate reads the value back for up to five seconds until it matches the
requested value. If the camera does not apply the change, the call returns `Error::NotApplied`.
Writes are not retried automatically.

## Vendor writes are rate-limited

The camera silently drops writes less than about 45 ms apart. The crate enforces a minimum 100 ms
interval, starting when the camera is opened. Calls that change multiple settings take
proportionally longer.

## Some writes still fail

In testing, roughly one in fifteen exposure writes was ignored despite adequate spacing, with no
clear pattern. These failures return `Error::NotApplied`. Retrying succeeded in testing.

## Position and zoom reflect the camera's actual state

Standard UVC controls return only the last requested values. They do not reflect changes from
tracking, gestures, Smart Composition, or manual movement. The crate reads actual values from vendor
selectors when available.

## Pan and tilt are set together

The camera accepts pan and tilt only as a pair. To change one axis, the crate first reads the
current position. If that position is implausible, the call returns `Error::PositionUnknown`.
Setting both axes still works.

## Some modes control the gimbal

DeskView uses its last saved angle, while overhead mode points straight down. Both restore the
previous position on exit. Moving the gimbal in DeskView updates its saved angle. Tracking and Auto
Framing follow people and do not restore the previous position on exit. Whiteboard mode leaves the
gimbal unchanged.

Mode requests also include a target position and zoom. The crate uses Link Controller's values for
automatic positioning; zeros would center the gimbal on every mode change.

## Privacy mode parks the gimbal

In privacy mode, the camera faces down, replaces video with a placeholder, and ignores gimbal
movement commands. Disabling privacy mode restores the previous position.

On Link 2, `Camera::privacy` reads a view-state flag. A downward-facing gimbal alone does not
indicate privacy mode: overhead mode, movement commands, and an inactive stream can also point the
gimbal down. Privacy mode is unsupported on cameras without this flag and on the original Link with
product ID `4c01`, whose flag Link Controller does not read.

## Presets are stored on the camera and applied by the host

The camera has ten preset slots, each holding a name, mode, gimbal position, and zoom.
`Camera::recall_preset` reads and applies a stored record. Reading a slot requires a write to select
it, so presets require an active video stream. Records use Link Controller's layout, but their
visibility in that application has not been tested.

## Mode transitions can return `Error::Busy`

During a mode transition, the camera may briefly report no mode. `Camera::mode` then returns
`Error::Busy`. Like Link Controller, the crate ignores these readings when checking mode changes.

When tracking is requested with no person in view, the camera remains busy and reports its tracking
switch as off until someone appears. `Camera::set_mode` with `CameraMode::Tracking` succeeds when
the request is accepted, but `Camera::mode` returns `CameraMode::Tracking` only when the camera has
someone to follow.

## Mode status reports progress and failures

`Camera::mode_status` reports whiteboard search, detection, and failure, as well as DeskView startup
failure. The camera may continue reporting a status after leaving the corresponding mode.

## Pause-track areas combine a rectangle and a gimbal angle

Each zone combines a rectangle, expressed as fractions of the frame from the top-left corner, with
the gimbal angle at capture time. This matches Link Controller's format. The camera uses both to
determine the zone's direction. Zones are numbered from 1 in input order and enabled with a separate
switch.

When the tracked person enters a zone, the camera turns toward the zone's center and holds until the
person leaves, then resumes tracking. In testing, a zone covering the left half of the view at pan
0° consistently held the camera at pan -19.5°, tilt 0°.

## Tracking area limits are a Link 2 Pro feature

Link 2 exposes the selector but ignores writes, which the crate reports as an error.

## Manual exposure requires ISO and shutter speed

The camera's stored manual values can be inaccurate, so `Exposure::Manual` requires both ISO and
shutter speed. The `Exposure` enum separates automatic exposure compensation from manual ISO and
shutter settings. Applying an exposure setting also selects its mode. To switch to manual exposure
while preserving the image, use the values from `Camera::exposure_in_effect`.

## Flips take a few seconds

Horizontal and vertical flips take a few seconds to appear in video, although their settings read
back immediately.
