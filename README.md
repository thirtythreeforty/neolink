# Neolink

Neolink is a small program that acts as a proxy between Reolink IP cameras and normal RTSP clients.
Certain cameras, such as the Reolink B800, do not implement ONVIF or RTSP, but instead use a proprietary protocol only compatible with their apps and NVRs (any camera that uses "port 9000" will likely be using this protocol).

Neolink allows you to use NVR software such as ZoneMinder or Blue Iris to receive video from these cameras instead.
Your NVR software connects to Neolink, which forwards the video stream from the camera.

The Neolink project is not affiliated with Reolink in any way; everything it does has been reverse engineered.

## Installation

In the future Neolink will be much easier to install.
Currently you need to install Gstreamer, including gst-rtsp-server, then build this project.

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

## License

Neolink is free software, released under the GNU Affero General Public License v3.
This means that if you incorporate it into a piece of software available over the network, you must be prepared to offer that software's source code to your users.
