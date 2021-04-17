# Neolink

![CI](https://github.com/thirtythreeforty/neolink/workflows/CI/badge.svg)

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

## Supported cameras

Neolink intends to support all Reolink cameras that do not provide native RTSP.
Currently it has been tested on the following cameras:

- B800/D800
- B400/D400
- E1
- Lumus

Neolink does not support other cameras such as the RLC-420, since they already
[provide native RTSP](https://support.reolink.com/hc/en-us/articles/360007010473-How-to-Live-View-Reolink-Cameras-via-VLC-Media-Player).

## Installation

In the future Neolink will be much easier to install.
For now, follow these steps.
Builds are provided for the following platforms:

- Windows x86_64 ([download][win-ci-download])
- macOS x86_64 ([download][macos-ci-download])
- Ubuntu x86_64 ([download][ubuntu-ci-download])
- Debian x86 ([download][debian-x86-ci-download])
- Debian aarch64 ([download][debian-aarch-ci-download])
- Debian armhf ([download][debian-armhf-ci-download])
- Docker x86 (see below)

### Windows/Linux

1. [Install Gstreamer][gstreamer] from the most recent MSI installer on Windows,
or your package manager on Linux.

2. If you are using Windows, add the following to your `PATH` environment variable:

    ```
    %GSTREAMER_1_0_ROOT_X86_64%\bin
    ```

    **Note:** If you use Chocolatey to install Gstreamer, it does this
    automatically.

3. Download and extract a [prebuilt binary from continuous integration][ci-download]
(click on the topmost commit for the most recent build).

3. Download and unpack Neolink from the links above.
   1. Note: you can also click on [this link][ci-download] to see all historical builds.
  You will need to be logged in to GitHub to download directly from the builds page.
4. Write a configuration file for your cameras.  See the section below.

5. Launch Neolink from a shell, passing your configuration file:

    ```bash
    neolink --config my_config.toml
    ```

6. Connect your NVR software to Neolink's RTSP server.

   The default URL is `rtsp://127.0.0.1:8554/your_camera_name` if you're running
   it on the same computer.
   If you run it on a different server, you may need to open firewall ports.
   See the "Viewing" section below for more troubleshooting.

[gstreamer]: https://gstreamer.freedesktop.org/documentation/installing/index.html
[ci-download]: https://github.com/thirtythreeforty/neolink/actions?query=workflow%3ACI+branch%3Amaster+

[win-ci-download]:          https://nightly.link/thirtythreeforty/neolink/workflows/build/master/release-windows-2019.zip
[macos-ci-download]:        https://nightly.link/thirtythreeforty/neolink/workflows/build/master/release-macos-10.15.zip
[ubuntu-ci-download]:       https://nightly.link/thirtythreeforty/neolink/workflows/build/master/release-ubuntu-18.04.zip
[debian-x86-ci-download]:   https://nightly.link/thirtythreeforty/neolink/workflows/build/master/release-i386-buster.zip
[debian-armhf-ci-download]: https://nightly.link/thirtythreeforty/neolink/workflows/build/master/release-armhf-buster.zip
[debian-aarch-ci-download]: https://nightly.link/thirtythreeforty/neolink/workflows/build/master/release-arm64-buster.zip

### Docker

A Docker image is also available containing Neolink and all its dependencies.
The image is `thirtythreeforty/neolink`.
Port 8554 is exposed, which is the default listen port.
You must mount a configuration file (see below) into the container at
`/etc/neolink.toml`.

Here is a sample launch commmand:

```bash
docker run \
  -p 8554:8554 \
  --restart=on-failure \
  --volume=$PWD/config.toml:/etc/neolink.toml \
  thirtythreeforty/neolink
```

The Docker image is "best effort" and intended for advanced users; questions
about running Docker are outside the scope of Neolink.

## Configuration

**Note**: for a more comprehensive setup tutorial, refer to the
[Blue Iris setup walkthrough in `docs/`][blue-iris-setup] (which is probably
  also helpful even with other NVR software).

[blue-iris-setup]: docs/Setting%20Up%20Neolink%20For%20Use%20With%20Blue%20Iris.md

Copy and modify the `sample_config.toml` to specify the address, username, and
password for each camera (if there is no password, you can omit that line).

Each `[[cameras]]` block creates a new camera; the `name` determines the RTSP
path you should connect your client to.
Currently Neolink cannot auto-detect cameras like the official clients do; you
must specify their IP addresses directly.

By default the H265 video format is used. Some cameras, for example E1, provide
H264 streams. To use these you must specify `format = "h264"` in the
`[[cameras]]` config. Soon this will be auto-detected, and you will not have to know or care about
the format.

By default, the HD stream is available at the RTSP path `/name` or
`/name/mainStream`, and the SD stream is available at `/name/subStream`.
You can use only the HD stream by adding `stream = "mainStream"` to the
`[[cameras]]` config, or only the SD stream with `stream = "subStream"`.

**Note**: The B400/D400 models only support a single stream at a time, so you
must add this line to sections for those cameras.

By default Neolink serves on all IP addresses on port 8554.
You can modify this by changing the `bind` and the `bind_port` parameter.
You only need one `bind`/`bind_port` setting at the top of the config file.

You can enable `rtsps` (TLS) by adding a `certificate = "/path/to/pem"` to the
top section of the config file. This PEM should contain by the certificate
and the key used for the server. If TLS is enabled all connections must use
`rtsps`. You can also control client side TLS with the config option
`tls_client_auth = "none|request|require"`; in this case the client should
present a certificate signed by the server's CA.

TLS is disabled by default.

You can password-protect the Neolink server by adding `[[users]]` sections to
the configuration file:

```
[[users]]
name: someone
pass: somepass
```

you also need to add the allowed users into each camera by adding the following
to `[[cameras]]`.

```
permitted_users = ["someone", "someoneelse"]
```

Anywhere a username is accepted it can take any username or one of the
following special values.

- `anyone` means any user with a valid user/pass
- `anonymous` means no user/pass required

The default `permitted_users` list is:

- `[ "anonymous"]` if no `[[users]]` were given in the config meaning no
authentication required to connect.

- `[ "anyone" ]` if `[[users]]` were provided meaning any authourised users can
connect.

You can change the Neolink log level by setting the `RUST_LOG` environment
variable (not in the configuration file) to one of `error`, `warn`, `info`,
`debug`, or `trace`:

```sh
set RUST_LOG=debug
```

On Linux:

```bash
export RUST_LOG=debug
```

## Viewing

Connect your RTSP client to the stream with the name you provided in the
configuration file.

Again, the default URL is `rtsp://127.0.0.1:8554/your_camera_name` if you're
running it on the same computer as the client.
The smaller SD video is `rtsp://127.0.0.1:8554/your_camera_name/subStream`.

4K cameras send large video "key frames" once every few seconds and the client
must have a receive buffer large enough to store the entire frame.
If your client's buffer size is configurable (like Blue Iris), ensure it's set
to 20MB, which should ensure plenty of headroom.

## Stability

Neolink has had minimal testing, but it seems to be very reliable in multiple
users' testing.

The formats of all configuration files and APIs is subject to change as required
while it is pre-1.0.

## Development

Neolink is written in Rust, and binds to Gstreamer to provide RTSP server
functionality.

To compile, ensure you have the Rust compiler, Gstreamer, and gst-rtsp-server
installed.

Then simply run:

```bash
cargo build
```

from this top directory.

### Baichuan Protocol

The "port 9000" protocol used by Reolink and some Swann cameras is internally
referred to as the Baichuan protocol; this is the company based in China that
is known internationally as Reolink.

This protocol is a slightly convoluted header-data format, and appears to have
been upgraded several times.

The modern variant uses obfuscated XML commands and sends ordinary H.265 or
H.264 video streams encapsulated in a custom header.

More details about the on-the-wire protocol will come later.

### Baichuan dissector

A Wireshark dissector is available for the BC wire protocol in the `dissector`
directory.

It dissects the BC header and also allows viewing the deobfuscated XML in
command messages.
To use it, copy or symlink it into your Wireshark plugin directory; typically
this is `~/.local/lib/wireshark/plugins/` under Linux.

Currently the dissector does not attempt to decode the Baichuan "extension"
messages except `binaryData`.
This will change in the future as reverse engineering needs require.

## License

Neolink is free software, released under the GNU Affero General Public License
v3.

This means that if you incorporate it into a piece of software available over
the network, you must offer that software's source code to your users.
