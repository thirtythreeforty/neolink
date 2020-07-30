# Setting up Neolink on Linux

This will go through the first steps of adding neolink onto a
linux based computer.

There are many flavours of linux so the name of some packages
may not exactly match. You are expected to be able to find the
correct names yourselves.

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

Here are some examples for command packages that work on some distros

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

# You will need this from aur, install yay first
sudo yay -S \
  gst-rtsp-server
```

## Downloading neolink

You now need to get a copy of neolink. You can get that from this
github under the tags tab.

Currently only amd64 is officially released, but armv7 will soon
be available too.

If you are installing on a raspberry pi, you will need to compile
from source until the official armv7 builds are available.

You can follow the following general terminal commands to download
and extract the v3.0 build.

```bash
curl -LJO 'https://github.com/thirtythreeforty/neolink/archive/v0.3.0.tar.gz'
tar xvf "neolink-0.3.0.tar.gz"
```

This will create a folder called `neolink-0.3.0` which contains
the `neolink` binary and a sample config `sample_config.toml`.
