# Neolink

![CI](https://github.com/QuantumEntangledAndy/neolink/workflows/CI/badge.svg)

Neolink is a small program that acts as a proxy between Reolink IP cameras and
normal RTSP clients.
Certain cameras, such as the Reolink B800, do not implement ONVIF or RTSP, but
instead use a proprietary "Baichuan" protocol only compatible with their apps
and NVRs (any camera that uses "port 9000" will likely be using this protocol).
Neolink allows you to use NVR software such as Shinobi or Blue Iris to receive
video from these cameras instead.
The Reolink NVR is not required, and the cameras are unmodified.
Your NVR software connects to Neolink, which forwards the video stream from the
camera.

The Neolink project is not affiliated with Reolink in any way; everything it
does has been reverse engineered.

## This Fork

This fork is an extension of [thirtythreeforty's](https://github.com/thirtythreeforty/neolink)
with additional features not yet in upstream master.

**Major Features**:

- MQTT
- Motion Detection
- Paused Streams (when no rtsp client or no motion detected)
- Save a still image to disk

**Minor Features**:

- Improved error messages when your missing gstreamer plugins
- Protocol more closely follows offical reolink format
  - Possibly can handle more simulatenous connections

## Installation

Download from the [release page](https://github.com/QuantumEntangledAndy/neolink/releases)

Extract the zip

Install the latest [gstreamer](https://gstreamer.freedesktop.org/download/) (1.20.5 as of writing this).
- **Windows**: ensure you install `full` when prompted in the MSI options.
- **Mac**: Install the dpkg version on the offical gstreamer website over the brew version
- **Ubuntu/Debian**: These packages should work
```bash
sudo apt install \
  libgstrtspserver-1.0-0 \
  libgstreamer1.0-0 \
  libgstreamer-plugins-bad1.0-0 \
  gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad
```

Make a config file see below.


## Config/Usage

### RTSP

To use `neolink` you need a config file.

There's a more complete example [here](https://github.com/QuantumEntangledAndy/neolink/blob/master/sample_config.toml),
but the following should work as a minimal example.

```toml
bind = "0.0.0.0"

[[cameras]]
name = "Camera01"
username = "admin"
password = "password"
uid = "ABCDEF0123456789"

[[cameras]]
name = "Camera02"
username = "admin"
password = "password"
uid = "BCDEF0123456789A"
```

Create a text file with called `neolink.toml` in the same folder as the neolink binary. With your config options.

When ready start `neolink` with the following command
using the terminal in the same folder the neolink binary is in.

```bash
./neolink rtsp --config=neolink.toml
```


### MQTT

To use mqtt you will to adjust your config file as such:

```toml
bind = "0.0.0.0"

[[cameras]]
name = "Camera01"
username = "admin"
password = "password"
uid = "ABCDEF0123456789"
  [cameras.mqtt]
  server = "127.0.0.1" # Address of the mqtt server
  port = 1883 # mqtt servers port
  credentials = ["username", "password"] # mqtt server login details
```

Then to start the mqtt connection run the following:

```bash
./neolink mqtt --config=neolink.toml
```

Neolink will publish these messages:

Messages are prefixed with `neolink/{CAMERANAME}`

Control messages:
- `/control/led [on|off]` Turns status LED on/off
- `/control/ir [on|off|auto]` Turn IR lights on/off or automatically via light detection
- `/control/reboot` Reboot the camera
- `/control/ptz` [up|down|left|right|in|out] (amount) Control the PTZ movements, amount defaults to 32.0

Status Messages:
`/status offline` Sent when the neolink goes offline this is a LastWill message
`/status disconnected` Sent when the camera goes offline
`/status/battery` Sent in reply to a `/query/battery`

Query Messages:
`/query/battery` Request that the camera reports its battery level (Not Yet Implemented)

### Pause

To use the pause feature you will need to adjust your config file as such:

```toml
bind = "0.0.0.0"

[[cameras]]
name = "Camera01"
username = "admin"
password = "password"
uid = "ABCDEF0123456789"
  [cameras.pause]
  on_motion = true # Should pause when no motion
  on_client = true # Should pause when no rtsp client
  mode = "none"  # What to do when paused values are: none, black, still, test
  timeout = 2.1 # How long to wait after motion stops before pausing
```

Then start the rtsp server as usual:

```bash
./neolink rtsp --config=neolink.toml
```


### Docker

[Docker](https://hub.docker.com/r/quantumentangledandy/neolink) builds are also provided
in multiple architectures. The latest tag tracks master while each branch gets it's own tag.

```bash
docker pull quantumentangledandy/neolink
```

### Image

You can write an image from the stream to disk using:


```bash
neolink image --config=config.toml --file-path=filepath CameraName
```

Where filepath is the path to save the image to and CameraName is the name of the camera
from the config to save the image from.

File is always jpeg and the extension given in filepath will be added or changed to reflect this.

## License

Neolink is free software, released under the GNU Affero General Public License
v3.

This means that if you incorporate it into a piece of software available over
the network, you must offer that software's source code to your users.
