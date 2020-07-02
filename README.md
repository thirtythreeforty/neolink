# Neolink

![CI](https://github.com/thirtythreeforty/neolink/workflows/CI/badge.svg)

Neolink is a small program that acts as a proxy between Reolink IP cameras and normal RTSP clients.
Certain cameras, such as the Reolink B800, do not implement ONVIF or RTSP, but instead use a proprietary "Baichuan" protocol only compatible with their apps and NVRs (any camera that uses "port 9000" will likely be using this protocol).

Neolink allows you to use NVR software such as Shinobi or Blue Iris to receive video from these cameras instead.
The Reolink NVR is not required, and the cameras are unmodified.
Your NVR software connects to Neolink, which forwards the video stream from the camera.

The Neolink project is not affiliated with Reolink in any way; everything it does has been reverse engineered.

## Supported cameras

Neolink intends to support all Reolink cameras that do not provide native RTSP.
Currently it has been tested on the following cameras:

- B800/D800

It *should* support the following cameras, but this has not yet been tested.
Please test if you have them, and [open an issue](https://github.com/thirtythreeforty/neolink/issues/new/choose) if you encounter problems:

- B400/D400
- E1
- Lumus

Neolink does not support other cameras such as the RLC-420, since they already [provide native RTSP](https://support.reolink.com/hc/en-us/articles/360007010473-How-to-Live-View-Reolink-Cameras-via-VLC-Media-Player).

## Installation

In the future Neolink will be much easier to install.
For now, follow these steps.

1. [Install Gstreamer][gstreamer] from the most recent MSI installer on Windows, or your package manager on Linux.
2. If you are using Windows, add the following to your `PATH` environment variable:

```
%GSTREAMER_1_0_ROOT_X86_64%\bin
```

Note that if you use Chocolatey to install Gstreamer, it does this automatically.

3. Download and extract a [prebuilt binary from continuous integration][ci-download] (click on the topmost commit for the most recent build).
4. Write a configuration file for your cameras.  See the section below.
5. Launch Neolink from a shell, passing your configuration file:

```
neolink --config my_config.toml
```

6. Connect your NVR software to Neolink's RTSP server.
   The default URL is `rtsp://127.0.0.1:8554/your_camera_name` if you're running it on the same computer.
   If you run it on a different server, you may need to open firewall ports.
   See the "Viewing" section below for more troubleshooting.

[gstreamer]: https://gstreamer.freedesktop.org/documentation/installing/index.html
[ci-download]: https://github.com/thirtythreeforty/neolink/actions?query=branch%3Amaster

## Configuration

Copy and modify the `sample_config.toml` to specify the address, username, and password for each camera (if there is no password, you can omit that line).
Each `[[cameras]]` block creates a new camera; the `name` determines the RTSP path you should connect your client to.
Currently Neolink cannot auto-detect cameras like the official clients do; you must specify their IP addresses directly.

By default the h265 streaming format is used. Some cameras, for example E1, provide h264 streams, to use these you must specify `format = "h264"` in the `[[cameras]]` config.

By default the HD stream is mounted at `name` and at `name/mainStream` and the SD stream is mounted at `name/subStream`. You can specify mounting only one by adding `stream = "subStream"` to the `[[cameras]]` config. You may also need to use `h264` format while using the SD stream.

By default Neolink serves on all IP addresses on port 8554.
You can modify this by changing the `bind` and the `bind_port` parameter.

You can change the Neolink log level by setting the `RUST_LOG` environment variable (not in the configuration file) to one of `error`, `warn`, `info`, `debug`, or `trace`:

```
set RUST_LOG=debug
```

On Linux:

```
export RUST_LOG=debug
```

## Viewing

Connect your RTSP client to the stream with the name you provided in the configuration file.
Again, the default URL is `rtsp://127.0.0.1:8554/your_camera_name` if you're running it on the same computer as the client.

4K cameras send large video "key frames" once every few seconds and the client must have a receive buffer large enough to store the entire frame.
If your client's buffer size is configurable (like Blue Iris), ensure it's set to 20MB, which should ensure plenty of headroom.

Note that some RTSP clients, especially VLC, have trouble handling the stream.
Currently this seems to be related to receive buffer sizes but the exact cause remains unclear.
Blue Iris is known to work well.

## Stability

Neolink has had minimal testing, but it seems to be very reliable in multiple users' testing.

The formats of all configuration files and APIs is subject to change as required while it is pre-1.0.

## Development

Neolink is written in Rust, and binds to Gstreamer to provide RTSP server functionality.
To compile, ensure you have the Rust compiler, Gstreamer, and gst-rtsp-server installed.
Then simply run:

```
cargo build
```

from this top directory.

### Baichuan Protocol

The "port 9000" protocol used by Reolink and some Swann cameras is internally referred to as the Baichuan protocol; this is the company based in China that is known internationally as Reolink.

This protocol is a slightly convoluted header-data format, and appears to have been upgraded several times.
The modern variant uses obfuscated XML commands and sends ordinary H.265 or H.264 video streams encapsulated in a custom header.
More details about the on-the-wire protocol will come later.

### Baichuan dissector

A Wireshark dissector is available for the BC wire protocol in the `dissector` directory.
It dissects the BC header and also allows viewing the deobfuscated XML in command messages.
To use it, copy or symlink it into your Wireshark plugin directory; typically this is `~/.local/lib/wireshark/plugins/` under Linux.

Currently the dissector does not attempt to decode the Baichuan "extension" messages except `binaryData`.
This will change in the future as reverse engineering needs require.

## License

Neolink is free software, released under the GNU Affero General Public License v3.
This means that if you incorporate it into a piece of software available over the network, you must offer that software's source code to your users.
