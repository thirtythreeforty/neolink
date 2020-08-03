# Setting up Neolink on Linux

This will go through the first steps of adding neolink onto a
linux based computer.

There are many flavours of linux so the name of some packages
may not exactly match. You are expected to be able to find the
correct names yourselves.

The general steps we will follow in this guide are as follows:

1. Install dependancies

2. Download neolink

3. Setup the config

4. Run neolink

## Installing the Dependencies

The dependencies for neolink include:

- Gstreamer RTSP server

- Gstreamer good plugin set

- Gstreamer bad plugin set

- glib 2.0

Glib2.0 is usually already installed on most modern unix flavours.
But the others will likely need to be installed via your package
manager.

Here are some examples for commands of package managers

- ubuntu, buster, stretch

  ```bash
  sudo apt install \
    libgstrtspserver-1.0-0 \
    libgstreamer1.0-0 \
    libgstreamer-plugins-bad1.0-0 \
    libgstreamer-plugins-good1.0-0 \
    gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad
  ```

- arch, manjaro

  ```bash
  sudo pacman -S \
    gstreamer \
    gst-plugins-bad \
    gst-plugins-good \

  # You will need this from AUR, and therefore need to install
  # yay first (see below if you haven't done this yet)
  sudo yay -S \
    gst-rtsp-server
  ```

  If your on archlinux or manjaro and you need to install `yay` do this:

  ```bash
  git clone https://aur.archlinux.org/yay.git
  cd yay
  makepkg -si
  ```

## Downloading neolink

You now need to get a copy of neolink. You can get that from this
github [under the CI assets](https://github.com/thirtythreeforty/neolink/actions?query=branch%3Amaster+workflow%3ACI).

Currently amd64 and armv7 are officially released.

If you are installing on a raspberry pi, you will need the armv7 build.

You will get a zip file called `release-ubuntu-18.04.zip`, unzip it with

```bash
unzip "release-ubuntu-18.04.zip"
```

This will extract two files called `neolink` and `neolink.d`. These are
the binaries we will be using.

## Setting up the Configuration File

Neolink uses a toml file for configuration. A sample toml can be found in the
source code called `sample_config.toml`.

You can download it with

```bash
curl -LJO 'https://github.com/thirtythreeforty/neolink/raw/master/sample_config.toml'
```

Open up the file in your favourite text editor.

For the most basic setup you need to change only the parts in `[[ cameras ]]`.
Neolink can connect to any number of supported reolink cameras at once. Each
camera requires its own `[[ cameras ]]` block. Here is an example one:

```toml
[[cameras]]
name = "driveway"
username = "admin"
password = "12345678"
address = "192.168.1.187:9000"
```

Set the `name` to any value you want, this will be the name of the rtsp stream.
In this tutorial I will assume you leave it as "driveway".

The username and passwords are the same ones used in the official reolink app.

The address is the ip address of the camera. You should set and use a fixed ip
for your camera. There are many ways to get a fixed ip address for your camera
including changing the settings in the reolink app.
Or by assigning a fixed DHCP address from your router. How to do either of
these things is beyond the scope of this tutorial.

- Note for E1 and Lumus

  If your camera is an E1 or a Lumus you will need to add the `format` line
  to your `[[ cameras ]]` block like this:

  ```toml
  [[cameras]]
  name = "driveway"
  username = "admin"
  password = "12345678"
  address = "192.168.1.187:9000"
  format = "h264"
  ```

  This will tell neolink that this camera uses the H264 format for its video.
  Future version of neolink will auto detect this.

Set up as many `[[ cameras ]]` as you want and save the file as `my_config.toml`

## Running Neolink

We are now ready to run neolink. Open up and terminal and navigate to the
folder where `neolink` and `my_config.toml` are.

Run neolink with:

```
./neolink --config my_config.toml
```

You should see messages such as:

```
[DATE TIME INFO  neolink] Neolink 0.3.0 (unknown commit) release
[DATE TIME INFO  neolink] camera: Connecting to camera at xxx.xxx.xxx.xxx:9000
[DATE TIME INFO  neolink] camera: Connecting to camera at xxx.xxx.xxx.xxx:9000
[DATE TIME INFO  neolink] camera: Connected to camera, starting video stream mainStream
[DATE TIME INFO  neolink] camera: Connected to camera, starting video stream subStream
```

Neolink should now be running :)


## Testing with ffprobe

To test neolink we will use `ffprobe`.

If you do not have `ffmpeg` installed yet do it now through your package
manager:

```bash
sudo apt install ffmpeg
```

You must leave neolink running at the same time you run ffprobe. Neolink is a
translator and it cannot translate reolink -> rtsp if you close it. The easiest
way to do this is to open two terminals. One with neolink running and the other
for running ffprobe.

In a new terminal run:

```bash
ffprobe rtsp://127.0.0.1:8554/driveway
```

Where you replace `driveway` with the name of your camera.

A successful run should report something like this at the end:

```
Input #0, rtsp, from 'rtsp://127.0.0.1:8554/driveway':
  Metadata:
    title           : Session streamed with GStreamer
    comment         : rtsp-server
  Duration: N/A, start: 0.114267, bitrate: N/A
    Stream #0:0: Video: h264 (High), yuv420p(progressive), 2304x1296, 90k tbr, 90k tbn, 180k tbc
```

You should also test the lower resolution subStream:

```bash
ffprobe rtsp://127.0.0.1:8554/driveway/subStream
```

If the mainStream fails but the subStream succeeds you may need to add
`format = "h264"` to your `[[ cameras ]]` section of your config.

## Common Errors

- Missing dependancies

  If when start neolink you get a message such as:

  ```
  libgstrtspserver-1.0.so.0: cannot open shared object file: No such file or directory
  ```

  You have not installed all of the dependencies repeat the `Installing the Dependencies`
  section of this guide.

- Not turned on

  If when using ffprobe to test you get:

  ```
  Connection to tcp://xxx.xxx.xxx.xxx:8554?timeout=0 failed: Connection refused
  rtsp://xxx.xxx.xxx.xxx:8554/driveway: Connection refused ```
  ```

  You have probably closed neolink. You should leave it running in an open
  terminal window whenever you want to use it. If you want to run it in the
  background, consider following the unix_service guide in the `docs/` folder
  of the source code.

- More advanced debugging required

  For other errors you will need to enable more debugging messages before
  running neolink for better diagnoses. To do this stop neolink and restart it
  with this command.

  ```bash
  GST_DEBUG=3 ./neolink --config my_config.toml
  ```

  This will print out a lot more information to the terminal. Try to connect
  to it with ffprobe and read the error messages it generates.

  - Advanced debugging: Missing plugins

    If while running advanced debugging you get a message like this:

    ```
    GST_ELEMENT_FACTORY gstelementfactory.c:456:gst_element_factory_make: no such element factory "h265parse"!
    ```

    This means that you are missing the gstreamer plugins. You must have both
    the good and the bad set of plugins. Please repeat the
    `Installing the Dependencies` section of this guide.

  - Advanced debugging: Incorrect video format

    If while running advanced debugging you get a message like this:

    ```
    h265parse gsth265parse.c:1110:gst_h265_parse_handle_frame:<h265parse0> broken/invalid nal Type: 48 Invalid, Size: 1073 will be dropped
    ```

    You have not set the format to `h264` in the `[[ cameras ]]` config while
    using an E1 or Lumus camera.

    Change the `[[ cameras ]]` config to include `format = "h264"` and restart
    neolink

    (If you are using another camera and you need to use h264 please let us
      know via an issue so we can update this guide)

- Something else

  If you still have problems open an issue on github and attach the
  neolink log. You can save the log to file with the following command:

  ```bash
  GST_DEBUG=3 ./neolink --config my_config.toml 2>&1 > neolink.log
  ```
